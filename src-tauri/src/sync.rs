use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct RemoteProduk {
    pub id: i64,
    pub nama: String,
    pub harga: i64,
    #[serde(default)]
    pub stok: i64,
    #[serde(default)]
    pub kategori_id: Option<i64>,
    #[serde(default)]
    pub barcode: Option<String>,
    #[serde(default)]
    pub foto_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RemoteKategori {
    pub id: i64,
    pub nama: String,
}

#[derive(Debug, Deserialize)]
pub struct RemoteMember {
    pub id: i64,
    pub nama: String,
    #[serde(default)]
    pub telepon: Option<String>,
    #[serde(default)]
    pub kategori_member_id: Option<i64>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct RemoteKategoriMember {
    pub id: i64,
    pub nama: String,
    #[serde(default)]
    pub diskon_persen: f64,
}

#[derive(Debug, Deserialize)]
pub struct RemoteUser {
    pub id: i64,
    pub toko_id: i64,
    pub nama: String,
    pub email: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub aktif: bool,
    #[serde(default)]
    pub kasir_pin_hash: String,
}

// Balik seperti endpoint `/api/auth/kasir-setup`: `{ toko_id, toko_nama?, users }`.
#[derive(Debug, Deserialize)]
pub struct RemoteUsersResp {
    pub toko_id: i64,
    #[serde(default)]
    pub toko_nama: Option<String>,
    pub users: Vec<RemoteUser>,
}

pub struct SyncClient {
    pub base: String,
    pub token: String,
    // blocking: sync_remote dijalankan Tauri di worker thread (command non-async),
    // jadi tak ada guard Mutex tersangkut lintas `.await` → future tetap Send.
    http: reqwest::blocking::Client,
}

impl SyncClient {
    pub fn new(base: String, token: String) -> Self {
        Self { base, token, http: reqwest::blocking::Client::new() }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base.trim_end_matches('/'), path)
    }

    // Server zpos percaya cookie `zpos_token` (getTokoFromRequest baca cookie, BUKAN
    // header Authorization/Bearer). Jadi kita kirim token via Cookie header.
    // JWT = base64url (huruf, angka, -, _, .) — aman utk value cookie tanpa encode.
    fn auth_cookie(&self) -> String {
        format!("zpos_token={}", self.token)
    }

    // Pesan error diagnosa: status + sampel body server (biar ketahuan 401/404/500 & pesannya).
    // Mengambil resp by value (resp.text() consume) — panggil HANYA di branch return.
    fn err_detail(&self, resp: reqwest::blocking::Response) -> String {
        let status = resp.status();
        let body = match resp.text() {
            Ok(b) => {
                let b = b.trim();
                if b.len() > 300 { format!("{}…", &b[..300]) } else { b.to_string() }
            }
            Err(_) => String::new(),
        };
        if body.is_empty() { format!("HTTP {status}") } else { format!("HTTP {status}: {body}") }
    }

    // Katalog produk dari server → upsert ke SQLite. Produk yang sudah hilang
    // dari server dibiarkan (kasir boleh tetap menjual stok lama) — sinkron
    // penuh (hapus di lokal) cukup lewat cara lain; `ponytail:` fitur itu.
    pub fn pull_produk(&self, conn: &mut Connection) -> Result<usize, String> {
        let resp = self.http.get(self.endpoint("/api/produk?semua=1"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        let list: Vec<RemoteProduk> = resp.json().map_err(|e| format!("json: {e}"))?;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO produk (id,nama,harga,stok,kategori_id,barcode,foto_url,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                   nama=excluded.nama, harga=excluded.harga, stok=excluded.stok,
                   kategori_id=excluded.kategori_id, barcode=excluded.barcode,
                   foto_url=excluded.foto_url, updated_at=datetime('now')",
            ).map_err(|e| e.to_string())?;
            for p in &list {
                st.execute((
                    p.id, &p.nama, p.harga, p.stok, p.kategori_id, &p.barcode, &p.foto_url,
                )).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(list.len())
    }

    // Kategori produk dari server → upsert (id, nama). Dipakai list_produk utk
    // join nama kategori → frontend tampilkan ikon & filter kategori.
    pub fn pull_kategori(&self, conn: &mut Connection) -> Result<usize, String> {
        let resp = self.http.get(self.endpoint("/api/kategori"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        let list: Vec<RemoteKategori> = resp.json().map_err(|e| format!("json: {e}"))?;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO kategori (id,nama) VALUES (?1,?2)
                 ON CONFLICT(id) DO UPDATE SET nama=excluded.nama",
            ).map_err(|e| e.to_string())?;
            for k in &list {
                st.execute((k.id, &k.nama)).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(list.len())
    }

    // Member + kategori member dari server → upsert.
    pub fn pull_member(&self, conn: &mut Connection) -> Result<usize, String> {
        let resp = self.http.get(self.endpoint("/api/member"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        let list: Vec<RemoteMember> = resp.json().map_err(|e| format!("json: {e}"))?;

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO member (id,nama,telepon,kategori_member_id)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(id) DO UPDATE SET
                   nama=excluded.nama, telepon=excluded.telepon,
                   kategori_member_id=excluded.kategori_member_id",
            ).map_err(|e| e.to_string())?;
            for m in &list {
                st.execute((m.id, &m.nama, &m.telepon, m.kategori_member_id))
                    .map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(list.len())
    }

    // Daftar kategori member toko (kasir & admin boleh lihat). Dipakai dropdown
    // saat kasir mendaftarkan member baru.
    pub fn list_kategori_member(&self) -> Result<Vec<RemoteKategoriMember>, String> {
        let resp = self.http.get(self.endpoint("/api/kategori-member"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        resp.json().map_err(|e| format!("json: {e}"))
    }

    // Simpan daftar user ke users_lokal (ganti isi penuh, tanpa ganti baris).
    fn store_users(&self, conn: &mut Connection, body: &RemoteUsersResp) -> Result<(), String> {
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            // Ganti isi tabel penuh: hapus semua dulu, lalu insert ulang dari server.
            // (hapus user yg akunnya tak lagi di daftar toko / dihapus penuh)
            tx.execute("DELETE FROM users_lokal", []).map_err(|e| e.to_string())?;
            let mut st = tx.prepare_cached(
                "INSERT INTO users_lokal (id,toko_id,nama,email,role,aktif,pin_hash)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
            ).map_err(|e| e.to_string())?;
            for u in &body.users {
                st.execute((
                    u.id, u.toko_id, &u.nama, &u.email, &u.role,
                    u.aktif as i64, &u.kasir_pin_hash,
                )).map_err(|e| e.to_string())?;
            }
            // Setiap tarik daftar user = reset semua attempt-lock PIN.
            tx.execute("DELETE FROM meta WHERE k LIKE 'pin_fail_%'", []).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    // Daftar user toko (utk login PIN offline) → store_users.
    // Endpoint admin-only `/api/auth/users`; token yg dipakai sync harus admin.
    pub fn pull_users(&self, conn: &mut Connection) -> Result<usize, String> {
        let resp = self.http.get(self.endpoint("/api/auth/users"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}: {}", resp.status().as_u16(),
                resp.text().unwrap_or_default().trim()));
        }
        let body: RemoteUsersResp = resp.json().map_err(|e| format!("json: {e}"))?;
        let n = body.users.len();
        self.store_users(conn, &body).map_err(|e| e.to_string())?;
        Ok(n)
    }

    // Setup pertama utk owner/admin tenant: login email+password sekali → server
    // validasi & balik daftar staff toko (auto-gen PIN utk yg belum punya).
    // Ganti jalur manual (tempel JWT admin). Tak butuh auth cookie — server
    // verifikasi kredensial sendiri di POST /api/auth/kasir-setup.
    pub fn setup_kasir(&self, conn: &mut Connection, email: &str, password: &str) -> Result<(usize, String), String> {
        let body = serde_json::json!({ "email": email, "password": password });
        let resp = self.http.post(self.endpoint("/api/auth/kasir-setup"))
            .json(&body)
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() {
            return Err(self.err_detail(resp));
        }
        // Backend balik { toko_id, toko_nama, token, users }. token = JWT access
        // utk app pakai sinkron katalog/member (sync_remote) tanpa tempel manual.
        #[derive(serde::Deserialize)]
        struct SetupResp {
            toko_id: i64,
            #[serde(default)]
            toko_nama: Option<String>,
            #[serde(default)]
            token: Option<String>,
            users: Vec<RemoteUser>,
        }
        let rb: SetupResp = resp.json().map_err(|e| format!("json: {e}"))?;
        let n = rb.users.len();
        let toko_nama = rb.toko_nama.clone().unwrap_or_default();
        {
            let users = RemoteUsersResp { toko_id: rb.toko_id, toko_nama: rb.toko_nama, users: rb.users };
            self.store_users(conn, &users).map_err(|e| e.to_string())?;
        }
        // Simpan JWT ke meta utk sync katalog/member berikutnya (sync_remote baca dari sini).
        if let Some(t) = &rb.token {
            conn.execute(
                "INSERT INTO meta (k,v) VALUES ('token_jwt',?1)
                 ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                [t],
            ).map_err(|e| e.to_string())?;
        }
        Ok((n, toko_nama))
    }

    // Tambah member dari kasir (titik jual). POST ke server; server balik member
    // yg sudah dibuat (dgn id). Insert id ke SQLite lokal supaya konsisten dgn
    // pull_member nanti. Offline HARUS dihindari di frontend (butuh server utk
    // alokasi id) — di sini error network ditolak, bukan diantri.
    pub fn tambah_member(&self, conn: &mut Connection, nama: &str, telepon: &str, kategori_member_id: Option<i64>) -> Result<String, String> {
        let body = serde_json::json!({
            "nama": nama,
            "telepon": if telepon.trim().is_empty() { Value::Null } else { Value::String(telepon.trim().to_string()) },
            "kategori_member_id": kategori_member_id
        });
        let resp = self.http.post(self.endpoint("/api/member"))
            .header("Cookie", self.auth_cookie())
            .json(&body)
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        #[derive(serde::Deserialize)]
        struct NewMember { id: i64, nama: String, telepon: Option<String>, kategori_member_id: Option<i64> }
        let m: NewMember = resp.json().map_err(|e| format!("json: {e}"))?;
        conn.execute(
            "INSERT INTO member (id,nama,telepon,kategori_member_id) VALUES (?1,?2,?3,?4)
             ON CONFLICT(id) DO UPDATE SET nama=excluded.nama, telepon=excluded.telepon,
               kategori_member_id=excluded.kategori_member_id",
            rusqlite::params![m.id, &m.nama, &m.telepon, m.kategori_member_id],
        ).map_err(|e| e.to_string())?;
        Ok(m.nama)
    }

    // Kirim semua transaksi yang antri offline ke server. Sukses → hapus antrian.
    // client_ref di server mencegah duplikat jika request pas tiba saat koneksi
    // terputus (idempotency — cara yang sama dipakai ZPos web).
    pub fn push_antrian(&self, conn: &mut Connection) -> Result<usize, String> {
        type Row = (i64, String, String, String, i64, String); // id, client_ref, produk, metode, total, dibuat_at
        let rows: Vec<Row> = {
            let mut st = conn.prepare(
                "SELECT id, client_ref, produk, metode, total, dibuat_at FROM antrian ORDER BY id",
            ).map_err(|e| e.to_string())?;
            let iter = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
                .map_err(|e| e.to_string())?;
            iter.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };

        let mut pushed = 0usize;
        for (id, client_ref, produk, _metode, _total, _dibuat) in rows {
            // Kolom `produk` kini menyimpan payload penuh { trx, items } (persis body
            // yang dikirim web ZPos ke POST /api/transaksi). Kirim apa adanya — server
            // yang validasi. Bila masih antrian lama berbentuk [{id,qty,harga}] (pra-format
            // ini), tidak akan cocok shape server dan ditolak — pengguna cukup re-entry.
            let body: Value = serde_json::from_str(&produk)
                .map_err(|e| format!("parse antrian {client_ref}: {e}"))?;
            let resp = self.http.post(self.endpoint("/api/transaksi"))
                .header("Cookie", self.auth_cookie())
                .json(&body)
                .send().map_err(|e| format!("network: {e}"))?;
            // 409 = duplikat (sudah pernah masuk) → anggap sukses, hapus antrian.
            if resp.status().is_success() || resp.status().as_u16() == 409 {
                conn.execute("DELETE FROM antrian WHERE id = ?1", [id]).map_err(|e| e.to_string())?;
                pushed += 1;
            } else {
                return Err(format!("push {client_ref}: {}", self.err_detail(resp)));
            }
        }
        Ok(pushed)
    }
}
