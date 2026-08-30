use rusqlite::Connection;
use serde::{Deserialize, Serialize};
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
    pub barcode_internal: Option<String>,
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

// Server kirim diskon_persen sebagai string ("-7", bisa negatif = markup),
// sedangkan tipe lokal f64 (SQLite REAL). Terima string ATAU angka biar
// deserialize tak gagal → dropdown kategori member tetap terisi.
fn de_f64_str<'de, D: serde::Deserializer<'de>>(de: D) -> Result<f64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Num { F(f64), S(String) }
    match Num::deserialize(de) {
        Ok(Num::F(f)) => Ok(f),
        Ok(Num::S(s)) => s.trim().parse().map_err(serde::de::Error::custom),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Deserialize, serde::Serialize)]
pub struct RemoteKategoriMember {
    pub id: i64,
    pub nama: String,
    #[serde(default, deserialize_with = "de_f64_str")]
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

// Shift aktif utk kasir lokal (dipakai frontend utk banner + kirim shift_id).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ShiftAktif {
    pub id: i64,
    #[serde(default)]
    pub nomor_shift: Option<i64>,
    #[serde(default)]
    pub kasir_nama: String,
    #[serde(default)]
    pub modal_awal: i64,
    #[serde(default)]
    pub buka_at: String,
    // Shift lokal offline: dibuka saat tak ada jaringan → id negatif, belum ada di
    // server. Transaksi/kas keluar di bawahnya disimpan lokal; disinkronkan saat online.
    #[serde(default)]
    pub offline: bool,
}

// Rekap shift setelah tutup (dipakai frontend utk modal rekap).
// Web `/api/shift/[id]` balik key snake_case (s.buka_at, jumlah_transaksi, dll).
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ShiftRekap {
    pub id: i64,
    #[serde(default)]
    pub nomor_shift: Option<i64>,
    #[serde(default)]
    pub kasir_nama: String,
    #[serde(default)]
    pub modal_awal: i64,
    #[serde(default)]
    pub buka_at: String,
    #[serde(default)]
    pub tutup_at: String,
    #[serde(default)]
    pub jumlah_transaksi: i64,
    #[serde(default)]
    pub total_penjualan: i64,
    #[serde(default)]
    pub total_tunai: i64,
    #[serde(default)]
    pub total_qris: i64,
    #[serde(default)]
    pub total_transfer: i64,
    #[serde(default)]
    pub total_kas_keluar: i64,
}

// Saldo kas live utk shift (server hitung): modal + total_tunai − kas_keluar.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SaldoShift {
    #[serde(default)]
    pub modal_awal: i64,
    #[serde(default)]
    pub total_tunai: i64,
    #[serde(default)]
    pub total_kas_keluar: i64,
    #[serde(default)]
    pub saldo_kas: i64,
}

// Satu entri pengeluaran kas.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct KasKeluar {
    pub id: i64,
    #[serde(default)]
    pub shift_id: i64,
    #[serde(default)]
    pub kategori: String,
    #[serde(default)]
    pub nominal: i64,
    #[serde(default)]
    pub catatan: String,
    #[serde(default)]
    pub void: i64,
    #[serde(default)]
    pub dibuat_at: String,
}

pub struct SyncClient {
    pub base: String,
    pub token: String,
    // blocking: sync_remote dijalankan Tauri di worker thread (command non-async),
    // jadi tak ada guard Mutex tersangkut lintas `.await` → future tetap Send.
    http: reqwest::blocking::Client,
    // Client timeout PENDEK (3s) utk aksi optimistik berbalas-cepat: tandai_bon &
    // hapus_bon. `http` (10s) utk sync berat (pull katalog/member antrian). Pisah
    // supaya hapus/tandai bon di kasir nggak nunggu 10s saat web lambat.
    http_fast: reqwest::blocking::Client,
}

impl SyncClient {
    pub fn new(base: String, token: String) -> Self {
        // Timeout total eksplisit — CRITICAL. `reqwest::blocking` default TANPA
        // timeout (docs 0.12: `timeout` = "Default is no timeout"). Connect ke IP
        // non-responsif = SYN retry OS bisa hang bermenit-menit; body read tak ada
        // batas. Akibat offline→online: worker thread sync_remote blok sangat lama.
        // Catatan: `connect_timeout` TIDAK efektif di blocking (butuh tokio runtime),
        // jadi satusatunya batas andal = `.timeout()` total request.
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))  // total per-request = deteksi offline cepat
            .build()
            .expect("build sync http client");
        let http_fast = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .expect("build sync http_fast client");
        Self { base, token, http, http_fast }
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

    // Pesan error diagnosa: base server + status + sampel body server (biar
    // ketahuan 401/404/500, endpoint mana, & pesannya). Panggil HANYA di return.
    fn err_detail(&self, resp: reqwest::blocking::Response) -> String {
        let status = resp.status();
        let body = match resp.text() {
            Ok(b) => {
                let b = b.trim();
                if b.len() > 300 { format!("{}…", &b[..300]) } else { b.to_string() }
            }
            Err(_) => String::new(),
        };
        // Catatan: body `null` dgn 401 = endpoint auth-token (/api/auth/me) menolak
        // token_jwt (stale/JWT_SECRET beda) — bukan kredensial salah. Body JSON error
        // (mis "Email atau password salah") = kredensial. base ikut utk identifikasi server.
        if body.is_empty() { format!("HTTP {status} @ {}", self.base) }
        else { format!("HTTP {status} @ {}: {body}", self.base) }
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
                "INSERT INTO produk (id,nama,harga,stok,kategori_id,barcode,barcode_internal,foto_url,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                   nama=excluded.nama, harga=excluded.harga, stok=excluded.stok,
                   kategori_id=excluded.kategori_id, barcode=excluded.barcode,
                   barcode_internal=excluded.barcode_internal,
                   foto_url=excluded.foto_url, updated_at=datetime('now')",
            ).map_err(|e| e.to_string())?;
            for p in &list {
                st.execute((
                    p.id, &p.nama, p.harga, p.stok, p.kategori_id, &p.barcode, &p.barcode_internal, &p.foto_url,
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

    // Tarik daftar kategori member dari server → cache ke sqlite lokal (tabel
    // `kategori_member`). Konsisten pola `store_users`: hapus-isi-penuh biar
    // kategori yg dihapus admin di server ikut hilang di lokal. Server = truth.
    // Sebelumnya kategori member HANYA di-fetch live (dropdown online) & tak pernah
    // di-persist → LEFT JOIN `list_member` selalu null (nama kategori & diskon hilang).
    pub fn pull_kategori_member(&self, conn: &mut Connection) -> Result<usize, String> {
        let resp = self.http.get(self.endpoint("/api/kategori-member"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        // Body kosong (200, chunked 0-byte) = toko belum punya kategori → samakan kosong.
        let body = resp.text().map_err(|e| format!("body: {e}"))?;
        let list: Vec<RemoteKategoriMember> = if body.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&body).map_err(|e| format!("json: {e}"))?
        };

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM kategori_member", []).map_err(|e| e.to_string())?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO kategori_member (id,nama,diskon_persen,urutan)
                 VALUES (?1,?2,?3,0)",
            ).map_err(|e| e.to_string())?;
            for km in &list {
                st.execute((km.id, &km.nama, km.diskon_persen)).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(list.len())
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

    // Data lisensi toko dari `/api/auth/me` (status langganan, plan, expired).
    // Sumber tanggal = kolom `toko.langganan_sampai` di DB ZPos web (guard.ts
    // statusToko). Kasir simpan hasilnya di meta `lisensi` (JSON) biar bisa
    // dibaca offline. Endpoint ini pakai getTokoFromRequest (cookie token) — siapa
    // pun user toko yg valid boleh akses (kasir/admin). Token sync = admin.
    pub fn pull_license(&self, conn: &mut Connection) -> Result<String, String> {
        let resp = self.http.get(self.endpoint("/api/auth/me"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        #[derive(Deserialize)]
        struct Me {
            #[serde(default)]
            nama: String,
            #[serde(default)]
            plan: String,
            #[serde(default)]
            aktif: bool,
            #[serde(default)]
            expired: bool,
            #[serde(default)]
            langganan_sampai: Option<String>,
        }
        let me: Me = resp.json().map_err(|e| format!("json: {e}"))?;
        // Data header nota dr `/api/pengaturan` (admin isi di Pengaturan web):
        // alamat, telepon, catatan_struk. Rollback-friendly: kalau endpoint gagal
        // (mis role kasir tak berhak / versi lama), tetap simpan yg sudah ada.
        let mut alamat = String::new();
        let mut telepon = String::new();
        let mut catatan_struk = String::new();
        let mut desain_nota = String::new();
        {
            let pr = self.http.get(self.endpoint("/api/pengaturan"))
                .header("Cookie", self.auth_cookie())
                .send();
            if let Ok(pre) = pr {
                if pre.status().is_success() {
                    #[derive(Deserialize)]
                    struct Pr {
                        #[serde(default)] alamat: String,
                        #[serde(default)] telepon: String,
                        #[serde(default)] catatan_struk: String,
                        #[serde(default)] desain_nota: String,
                    }
                    if let Ok(p) = pre.json::<Pr>() {
                        alamat = p.alamat; telepon = p.telepon; catatan_struk = p.catatan_struk;
                        desain_nota = p.desain_nota;
                    }
                }
            }
        }
        let v = serde_json::json!({
            "nama": me.nama, "alamat": alamat, "telepon": telepon,
            "catatan_struk": catatan_struk, "desain_nota": desain_nota,
            "plan": me.plan, "aktif": me.aktif, "expired": me.expired,
            "langganan_sampai": me.langganan_sampai,
        }).to_string();
        conn.execute(
            "INSERT INTO meta (k,v) VALUES ('lisensi',?1)
             ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            [&v],
        ).map_err(|e| e.to_string())?;
        Ok(me.nama)
    }

    // Tarik daftar bon GANTUNG AKTIF dari web (GET /api/bon, tanpa ?semua → hanya
    // selesai=false) lalu simpan raw JSON ke meta `bon_sync`. Dipakai frontend utk
    // merge anti-duplikat (upsert by id, skip yg sudah ada di lokal). Bon yang uda
    // selesai TIDAK ditarik (kasir tak perlu). Item virtual id<0 tak ada di web
    // (server tolak) → aman utk pull.
    pub fn pull_bon(&self, conn: &mut Connection) -> Result<usize, String> {
        let resp = self.http.get(self.endpoint("/api/bon"))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        let body = resp.text().map_err(|e| format!("text: {e}"))?;
        let n: usize = serde_json::from_str::<Vec<serde_json::Value>>(&body)
            .map(|a| a.len())
            .unwrap_or(0);
        conn.execute(
            "INSERT INTO meta (k,v) VALUES ('bon_sync',?1)
             ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            [&body],
        ).map_err(|e| e.to_string())?;
        Ok(n)
    }

    // Simpan JSON shift aktif per user ke meta (utk baca offline oleh `ambil_shift`).
    fn store_shift(&self, conn: &mut Connection, user_id: i64, s: &ShiftAktif) -> Result<(), String> {
        let v = serde_json::to_string(s).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO meta (k,v) VALUES (?1,?2)
             ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            rusqlite::params![format!("shift_{user_id}"), &v],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    // Buka shift utk kasir lokal (`user_id` = id web user). Token sync = admin,
    // jd server meng-assign shift ke user tsb (admin-only di web POST /api/shift).
    pub fn buka_shift(&self, conn: &mut Connection, user_id: i64, modal: i64) -> Result<ShiftAktif, String> {
        // Offline (tak bisa hubungi server) → buka shift LOKAL dgn id negatif & flag
        // offline. Frontend kirim `SHIFT.id` (negatif) utk transaksi/kas keluar di
        // bawahnya; rekap dihitung dari data lokal saat tutup. Saat online, shift
        // lokal disinkronkan (posting ke server → id positif).
        let resp = match self.http.post(self.endpoint("/api/shift"))
            .header("Cookie", self.auth_cookie())
            .json(&serde_json::json!({ "modal_awal": modal, "user_id": user_id }))
            .send() {
            Ok(r) => r,
            Err(_) => {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() % 1_000_000).unwrap_or(0) as i64;
                let lid = -(user_id * 1_000_000 + t);
                let s = ShiftAktif {
                    id: lid, nomor_shift: None,
                    kasir_nama: format!("offline-{user_id}"),
                    modal_awal: modal, buka_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
                        .as_secs().to_string(),
                    offline: true,
                };
                self.store_shift(conn, user_id, &s)?;
                conn.execute(
                    "INSERT INTO meta (k,v) VALUES (?1,'1') ON CONFLICT(k) DO UPDATE SET v='1'",
                    [format!("shift_offline_{user_id}")],
                ).map_err(|e| e.to_string())?;
                return Ok(s);
            }
        };
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        let mut s: ShiftAktif = resp.json().map_err(|e| format!("json: {e}"))?;
        s.offline = false;
        self.store_shift(conn, user_id, &s)?;
        let _ = conn.execute("DELETE FROM meta WHERE k=?1", [format!("shift_offline_{user_id}")]);
        Ok(s)
    }

    // Tutup shift → server hitung rekap totals (dipakai modal rekap di frontend).
    // Hapus shift aktif lokal user tsb.
    pub fn tutup_shift(&self, conn: &mut Connection, user_id: i64, shift_id: i64) -> Result<Option<ShiftRekap>, String> {
        // Shift OFFLINE (id negatif, flag shift_offline) → hitung rekap dari data
        // LOKAL, tanpa hit server. Transaksi antrian yg punya shift_id==shift_id ini
        // + kas keluar lokal dijumlahkan. (Atribusi ke shift server saat online
        // ditangani Fase 2 sync shift — Fase 1 cuma bikin shift+rekap offline jalan.)
        let is_offline: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE k=?1)",
            [format!("shift_offline_{user_id}")],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0) == 1;
        if shift_id < 0 || is_offline {
            let modal: i64 = conn.query_row(
                "SELECT COALESCE(json_extract(v,'$.modal_awal'),0) FROM meta WHERE k=?1",
                [format!("shift_{user_id}")],
                |r| r.get(0),
            ).unwrap_or(0);
            // Rekap dari transaksi antrian yg shift_id-nya == shift lokal ini.
            let mut total_penjualan = 0i64;
            let mut total_tunai = 0i64;
            let mut jumlah = 0i64;
            {
                let mut st = conn.prepare("SELECT produk, total, metode FROM antrian").map_err(|e| e.to_string())?;
                let rows = st.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,Option<String>>(2)?)))
                    .map_err(|e| e.to_string())?;
                for row in rows {
                    let (produk, total, metode) = row.map_err(|e| e.to_string())?;
                    // hanya transaksi milik shift lokal ini: payload trx.shift_id == shift_id
                    let sid: Option<i64> = serde_json::from_str::<serde_json::Value>(&produk)
                        .ok().and_then(|v| v.get("trx").and_then(|t| t.get("shift_id")).and_then(|x| x.as_i64()));
                    if sid != Some(shift_id) { continue; }
                    jumlah += 1;
                    total_penjualan += total;
                    if metode.as_deref() == Some("Tunai") { total_tunai += total; }
                }
            }
            // Kas keluar lokal shift ini dari meta (array entri per-item offline).
            let total_kas_keluar: i64 = {
                let kk_key = format!("kas_offline_{user_id}_{shift_id}");
                let arr: Vec<Value> = conn.query_row(
                    "SELECT COALESCE(v,'[]') FROM meta WHERE k=?1",
                    [&kk_key], |r| r.get::<_, String>(0),
                ).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                // Kompat data lama (objek {"total":N}) → per-entri.
                let arr: Vec<Value> =
                    if arr.len() == 1 && arr[0].get("total").is_some() {
                        vec![serde_json::json!({ "nominal": arr[0].get("total").and_then(|x| x.as_i64()).unwrap_or(0) })]
                    } else { arr };
                arr.iter().filter_map(|e| e.get("nominal").and_then(|x| x.as_i64())).sum()
            };
            let r = ShiftRekap {
                id: shift_id, nomor_shift: None, kasir_nama: format!("offline-{user_id}"),
                modal_awal: modal,
                buka_at: conn.query_row(
                    "SELECT COALESCE(json_extract(v,'$.buka_at'),'') FROM meta WHERE k=?1",
                    [format!("shift_{user_id}")], |r| r.get(0),
                ).unwrap_or_default(),
                tutup_at: String::new(),
                jumlah_transaksi: jumlah, total_penjualan, total_tunai,
                total_qris: 0, total_transfer: 0, total_kas_keluar,
            };
            // Catat shift offline utk di-replay ke server saat online (Fase 2):
            // {old_id, user_id, modal_awal, buka_at}. `tutup_at` server hitung sendiri.
            let kas_total = total_kas_keluar;
            let buka_str: String = conn.query_row(
                "SELECT COALESCE(json_extract(v,'$.buka_at'),'') FROM meta WHERE k=?1",
                [format!("shift_{user_id}")], |r| r.get(0),
            ).unwrap_or_default();
            let mut pending: Vec<serde_json::Value> = conn.query_row(
                "SELECT COALESCE(v,'[]') FROM meta WHERE k='shift_sync_pending'",
                [], |r| r.get::<_, String>(0),
            ).and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e))))
             .unwrap_or_default();
            pending.push(serde_json::json!({
                "user_id": user_id,
                "old_id": shift_id,
                "modal_awal": modal,
                // `buka_at` simpan utk dipakai replay. Kalau berupa epoch detik (Fase 1)
                // → server butuh ISO; konversi di titik replay (sync_replay_shift).
                "buka_at": buka_str,
                "kas_offline": kas_total,
            }));
            conn.execute(
                "INSERT INTO meta (k,v) VALUES ('shift_sync_pending',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                [serde_json::to_string(&pending).map_err(|e| e.to_string())?],
            ).map_err(|e| e.to_string())?;
            let _ = conn.execute("DELETE FROM meta WHERE k IN (?1,?2)", rusqlite::params![
                format!("shift_{user_id}"), format!("shift_offline_{user_id}"),
            ]);
            return Ok(Some(r));
        }
        let resp = self.http.patch(self.endpoint(&format!("/api/shift/{shift_id}")))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        let r: ShiftRekap = resp.json().map_err(|e| format!("json: {e}"))?;
        let _ = conn.execute("DELETE FROM meta WHERE k=?1", [format!("shift_{user_id}")]);
        Ok(Some(r))
    }

    // Saldo kas live utk shift: server hitung modal + total_tunai − kas_keluar.
    // Pakai GET `/api/shift/{id}` (detail rekap). Offline → Err (frontend cache).
    pub fn saldo_shift(&self, shift_id: i64) -> Result<SaldoShift, String> {
        let resp = self.http.get(self.endpoint(&format!("/api/shift/{shift_id}")))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)] modal_awal: i64,
            #[serde(default)] total_tunai: i64,
            #[serde(default)] total_kas_keluar: i64,
        }
        let r: Raw = resp.json().map_err(|e| format!("json: {e}"))?;
        Ok(SaldoShift {
            modal_awal: r.modal_awal,
            total_tunai: r.total_tunai,
            total_kas_keluar: r.total_kas_keluar,
            saldo_kas: r.modal_awal + r.total_tunai - r.total_kas_keluar,
        })
    }

    // Saldo kas live utk shift OFFLINE (id negatif): hitung dari data LOKAL (SQLite)
    // — modal_awal (meta shift_{user}) + tunai (antrian trx.shift_id==shift_id) −
    // kas keluar (meta kas_offline_{user}_{shift}). Biar badge/modal saldo tetap
    // kelihatan saat buka shift offline, bukan cuma bergantung cache server `saldo_shift`.
    pub fn saldo_shift_offline(&self, conn: &mut Connection, user_id: i64, shift_id: i64) -> Result<SaldoShift, String> {
        let modal: i64 = conn.query_row(
            "SELECT COALESCE(json_extract(v,'$.modal_awal'),0) FROM meta WHERE k=?1",
            [format!("shift_{user_id}")],
            |r| r.get(0),
        ).unwrap_or(0);
        let mut total_tunai = 0i64;
        {
            let mut st = conn.prepare("SELECT produk, total, metode FROM antrian").map_err(|e| e.to_string())?;
            let rows = st.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,Option<String>>(2)?)))
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (produk, total, metode) = row.map_err(|e| e.to_string())?;
                if metode.as_deref() != Some("Tunai") { continue; }
                let sid: Option<i64> = serde_json::from_str::<serde_json::Value>(&produk)
                    .ok().and_then(|v| v.get("trx").and_then(|t| t.get("shift_id")).and_then(|x| x.as_i64()));
                if sid != Some(shift_id) { continue; }
                total_tunai += total;
            }
        }
        let kk_key = format!("kas_offline_{user_id}_{shift_id}");
        let arr: Vec<Value> = conn.query_row(
            "SELECT COALESCE(v,'[]') FROM meta WHERE k=?1",
            [&kk_key], |r| r.get::<_, String>(0),
        ).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let arr: Vec<Value> =
            if arr.len() == 1 && arr[0].get("total").is_some() {
                vec![serde_json::json!({ "nominal": arr[0].get("total").and_then(|x| x.as_i64()).unwrap_or(0) })]
            } else { arr };
        let total_kas_keluar: i64 = arr.iter().filter_map(|e| e.get("nominal").and_then(|x| x.as_i64())).sum();
        Ok(SaldoShift {
            modal_awal: modal,
            total_tunai,
            total_kas_keluar,
            saldo_kas: modal + total_tunai - total_kas_keluar,
        })
    }

    // Catat pengeluaran kas (kas keluar) utk shift → POST /api/kas-keluar.
    // Kasir lokal: user_id = id web kasir yg login; server assign entri ke dia.
    pub fn kirim_kas_keluar(&self, conn: &mut Connection, shift_id: i64, user_id: i64, kategori: &str, nominal: i64, catatan: &str) -> Result<i64, String> {
        let body = serde_json::json!({
            "shift_id": shift_id, "user_id": user_id, "kategori": kategori,
            "nominal": nominal, "catatan": catatan,
        });
        let resp = match self.http.post(self.endpoint("/api/kas-keluar"))
            .header("Cookie", self.auth_cookie())
            .json(&body)
            .send() {
            Ok(r) => r,
            Err(_) => {
            // Offline → simpan entri per-item (kategori/nominal/catatan) utk shift ini
            // di meta (JSON array), supaya rekap shift offline akurat & saat online
            // di-replay per-entri (kategori/catatan asli tak hilang di laporan).
            let key = format!("kas_offline_{user_id}_{shift_id}");
            let cur: Vec<Value> = conn.query_row(
                "SELECT COALESCE(v,'[]') FROM meta WHERE k=?1",
                [&key], |r| r.get::<_, String>(0),
            ).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            let mut arr = cur;
            arr.push(serde_json::json!({
                "kategori": kategori, "nominal": nominal, "catatan": catatan,
            }));
            let arr_str = serde_json::to_string(&arr).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO meta (k,v) VALUES (?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                rusqlite::params![&key, &arr_str],
            ).map_err(|e| e.to_string())?;
            return Ok(-(shift_id.abs())); // id placeholder negatif; tak dipakai lanjut
            }
        };
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        #[derive(Deserialize)]
        struct New { id: i64 }
        let m: New = resp.json().map_err(|e| format!("json: {e}"))?;
        Ok(m.id)
    }

    // Daftar pengeluaran kas utk shift → GET /api/kas-keluar?shift_id=...
    pub fn daftar_kas_keluar(&self, shift_id: i64) -> Result<Vec<KasKeluar>, String> {
        let resp = self.http.get(self.endpoint(&format!("/api/kas-keluar?shift_id={shift_id}")))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        resp.json().map_err(|e| format!("json: {e}"))
    }

    // Cek shift aktif kasir dari server → refresh cache lokal. Best-effort.
    pub fn cek_shift(&self, conn: &mut Connection, user_id: i64) -> Result<Option<ShiftAktif>, String> {
        // Kalau shift LOKAL OFFLINE masih terbuka, JANGAN timpa/hapus dari server —
        // server tak tahu shift lokal; sync shift baru dilakukan di Fase 2.
        let offline_flag: i64 = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE k=?1)",
            [format!("shift_offline_{user_id}")], |r| r.get(0),
        ).unwrap_or(0);
        if offline_flag == 1 {
            let s: Option<ShiftAktif> = conn.query_row(
                "SELECT v FROM meta WHERE k=?1", [format!("shift_{user_id}")],
                |r| r.get::<_, String>(0),
            ).ok().and_then(|v| serde_json::from_str(&v).ok());
            return Ok(s);
        }
        let resp = self.http.get(self.endpoint(&format!("/api/shift/active?user_id={user_id}")))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() { return Err(self.err_detail(resp)); }
        #[derive(Deserialize)]
        struct Res { shift: Option<ShiftAktif> }
        let r: Res = resp.json().map_err(|e| format!("json: {e}"))?;
        match &r.shift {
            Some(s) => self.store_shift(conn, user_id, s)?,
            None => { let _ = conn.execute("DELETE FROM meta WHERE k=?1", [format!("shift_{user_id}")]); }
        }
        Ok(r.shift)
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
        // Replay shift OFFLINE yg ditutup (Fase 2) SEBELUM menaruh transaksi dgn
        // shift_id negatif, supaya server punya shift dengan id positif utk repoint.
        self.sync_replay_shift(conn)?;

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

    // Fase 2 — replay shift OFFLINE yg sudah ditutup ke server saat online:
    // (1) POST /api/shift dgn buka_at asli → dapat id positif server
    // (2) repoint semua antrian yg punya trx.shift_id==old_id(negatif) -> new_id
    // (3) kirim kas keluar offline (total) ke shift baru
    // (4) hapus dari pending + meta kas_offline.
    // Dipanggil di awal `push_antrian`, SEBELUM transaksi dgn shift_id lama di-push.
    pub fn sync_replay_shift(&self, conn: &mut Connection) -> Result<usize, String> {
        let raw: String = conn.query_row(
            "SELECT COALESCE(v,'[]') FROM meta WHERE k='shift_sync_pending'",
            [], |r| r.get::<_, String>(0),
        ).unwrap_or_else(|_| "[]".into());
        let mut pending: Vec<serde_json::Value> = match serde_json::from_str(&raw) {
            Ok(v) => v, Err(_) => return Ok(0),
        };
        if pending.is_empty() { return Ok(0); }

        let mut kept: Vec<serde_json::Value> = Vec::new();
        let mut replayed = 0usize;
        for p in pending {
            let user_id = p.get("user_id").and_then(|v| v.as_i64());
            let old_id = p.get("old_id").and_then(|v| v.as_i64());
            let Some(old_id) = old_id else { continue };
            let mut item = p;
            let modal = item.get("modal_awal").and_then(|v| v.as_i64()).unwrap_or(0);
            // Isolasi borrow: baca new_id dulu, baru bisa mutate `item` di arm None.
            let prev_new_id = item.get("new_id").and_then(|v| v.as_i64());

            // Kalau shift SUDAH dibuat (new_id tersimpan dari replay sebelumnya) →
            // jangan POST ulang (cegah duplikat shift server); cukup kirim sisa
            // kas keluar offline yang belum sempat terkirim.
            let new_id: i64 = match prev_new_id {
                Some(id) => id,
                None => {
                    // buka_at: bisa ISO ("2026-...") atau epoch detik (Fase 1) → konversi.
                    let mut buka_iso = item.get("buka_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if let Ok(secs) = buka_iso.trim().parse::<i64>() {
                        let d = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64);
                        let dt: chrono::DateTime<chrono::Utc> = d.into();
                        buka_iso = dt.to_rfc3339();
                    }
                    let body = serde_json::json!({
                        "modal_awal": modal,
                        "user_id": user_id,
                        "buka_at": if buka_iso.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(buka_iso) },
                    });
                    let resp = match self.http.post(self.endpoint("/api/shift"))
                        .header("Cookie", self.auth_cookie())
                        .json(&body)
                        .send() {
                        Ok(r) => r, Err(_) => { kept.push(item); continue; } // offline kembali, tahan
                    };
                    if !resp.status().is_success() { kept.push(item); continue; }
                    #[derive(serde::Deserialize)]
                    struct NewShift { id: i64 }
                    let Ok(ns) = resp.json::<NewShift>() else { kept.push(item); continue; };
                    let id = ns.id;

                    // Repoint antrian: trx.shift_id old_id(neg) -> new_id.
                    let rows: Vec<(i64, String)> = {
                        let mut st = conn.prepare("SELECT id, produk FROM antrian").map_err(|e| e.to_string())?;
                        let iter = st.query_map([], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))
                            .map_err(|e| e.to_string())?;
                        iter.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())?
                    };
                    for (aid, produk) in rows {
                        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&produk) {
                            if let Some(sid) = v.get("trx").and_then(|t| t.get("shift_id")).and_then(|x| x.as_i64()) {
                                if sid == old_id {
                                    v["trx"]["shift_id"] = serde_json::json!(id);
                                    let s = serde_json::to_string(&v).map_err(|e| e.to_string())?;
                                    conn.execute("UPDATE antrian SET produk=?1 WHERE id=?2", rusqlite::params![s, aid])
                                        .map_err(|e| e.to_string())?;
                                }
                            }
                        }
                    }
                    item["new_id"] = serde_json::json!(id);
                    id
                }
            };

            // Kirim kas keluar offline PER-ENTRI (kategori/catatan asli) ke shift baru.
            // done=true → semua terkirim (atau tak ada) → shift selesai.
            // done=false → masih ada entri gagal → retain item (dgn new_id) utk retry.
            let done = self.kirim_kas_keluar_offline(conn, user_id, old_id, new_id)?;
            if done { replayed += 1; } else { kept.push(item); }
        }
        conn.execute(
            "INSERT INTO meta (k,v) VALUES ('shift_sync_pending',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            [serde_json::to_string(&kept).map_err(|e| e.to_string())?],
        ).map_err(|e| e.to_string())?;
        Ok(replayed)
    }

    // Kirim kas keluar offline shift (array entri per-item) yg tersimpan di meta ke
    // shift server `new_id`. Entri sukses dihapus; kalau masih ada yg gagal (mis.
    // jaringan putus) array dipertahankan & method return false → shift item ditahan
    // utk retry. Ini juga cegah data kas keluar hilang dari server saat koneksi turun
    // di tengah replay (edge: shift sudah dibikin, kas keluar belum terkirim).
    fn kirim_kas_keluar_offline(&self, conn: &mut Connection, user_id: Option<i64>, old_id: i64, new_id: i64) -> Result<bool, String> {
        let Some(uid) = user_id else { return Ok(true); };
        let kk_key = format!("kas_offline_{uid}_{old_id}");
        let arr: Vec<serde_json::Value> = conn.query_row(
            "SELECT COALESCE(v,'[]') FROM meta WHERE k=?1",
            [&kk_key], |r| r.get::<_, String>(0),
        ).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        // Kompat: data lama (Fase 1) berbentuk objek {"total":N} → ubah jadi array
        // per-entri satu baris "Pengeluaran offline" biar tercatat (nominal tak hilang).
        let arr: Vec<serde_json::Value> = if arr.len() == 1 && arr[0].get("total").is_some() {
            let total = arr[0].get("total").and_then(|x| x.as_i64()).unwrap_or(0);
            vec![serde_json::json!({ "kategori": "Lainnya", "nominal": total, "catatan": "Pengeluaran offline" })]
        } else { arr };
        if arr.is_empty() { return Ok(true); }
        let mut remaining: Vec<serde_json::Value> = Vec::new();
        for e in arr {
            let kategori = e.get("kategori").and_then(|x| x.as_str()).unwrap_or("Lainnya").to_string();
            let nominal = e.get("nominal").and_then(|x| x.as_i64()).unwrap_or(0);
            let catatan = e.get("catatan").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let kb = serde_json::json!({
                "shift_id": new_id, "user_id": uid,
                "kategori": kategori, "nominal": nominal, "catatan": catatan,
            });
            let sent = match self.http.post(self.endpoint("/api/kas-keluar"))
                .header("Cookie", self.auth_cookie()).json(&kb).send() {
                Ok(r) => r.status().is_success(),
                Err(_) => false,
            };
            if !sent { remaining.push(e); }
        }
        if remaining.is_empty() {
            let _ = conn.execute("DELETE FROM meta WHERE k=?1", [&kk_key]);
            Ok(true)
        } else {
            let rem_str = serde_json::to_string(&remaining).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO meta (k,v) VALUES (?1,?2) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
                rusqlite::params![&kk_key, &rem_str],
            ).map_err(|e| e.to_string())?;
            Ok(false)
        }
    }

    /// Simpan bon gantung ke server (`/api/bon`) supaya tampil di Laporan web.
    /// `produk` = {"<produk_id>": qty} hanya utk produk ASLI (id>0); item virtual
    /// (id negatif) tidak bisa digantung ke server (bon web butuh ref produk).
    pub fn kirim_bon(&self, nama: &str, produk: &str, total: i64) -> Result<i64, String> {
        let body: Value = serde_json::json!({
            "nama": if nama.trim().is_empty() { serde_json::Value::Null } else { serde_json::Value::String(nama.to_string()) },
            "produk": serde_json::from_str::<Value>(produk).unwrap_or(Value::Object(Default::default())),
            "total": total,
        });
        let resp = self.http.post(self.endpoint("/api/bon"))
            .header("Cookie", self.auth_cookie())
            .json(&body)
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("bon: {}", self.err_detail(resp)));
        }
        let r: Value = resp.json().map_err(|e| format!("json: {e}"))?;
        r.get("id").and_then(|v| v.as_i64()).ok_or_else(|| "server tak kembalikan bon id".into())
    }

    /// Ubah isi bon yang sudah ada di server (`PATCH /api/bon/{id}`). Dipakai
    /// kasir saat bon ditarik → ditambah item → disimpan ulang. `produk` = FULL
    /// daftar item final ({"<produk_id>": qty}); web hitung delta stok-hold.
    /// 404 = bon tak ada → anggap gagal (kasir jangan timpa lokal tanpa server).
    pub fn edit_bon(&self, bon_id: i64, produk: &str, total: i64) -> Result<(), String> {
        let body: Value = serde_json::json!({
            "produk": serde_json::from_str::<Value>(produk).unwrap_or(Value::Object(Default::default())),
            "total": total,
        });
        let resp = self.http.patch(self.endpoint(&format!("/api/bon/{bon_id}")))
            .header("Cookie", self.auth_cookie())
            .json(&body)
            .send().map_err(|e| format!("network: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("edit bon {bon_id}: {}", self.err_detail(resp)));
        }
        Ok(())
    }

    /// Tandai bon selesai (dibayar) di server (`PATCH /api/bon/{id}`) biar tak
    /// mengambang aktif di tab Bon web saat bayar lewat windows.
    pub fn tandai_bon_selesai(&self, bon_id: i64) -> Result<(), String> {
        let resp = self.http_fast.patch(self.endpoint(&format!("/api/bon/{bon_id}")))
            .header("Cookie", self.auth_cookie())
            .json(&serde_json::json!({ "selesai": true }))
            .send().map_err(|e| format!("network: {e}"))?;
        // 404 = bon tak ada (mungkin uda dihapus dari web) → anggap tak masalah.
        if resp.status().is_success() || resp.status().as_u16() == 404 {
            Ok(())
        } else {
            Err(format!("tutup bon {}: {}", bon_id, self.err_detail(resp)))
        }
    }

    /// Hapus bon permanen di server (`DELETE /api/bon/{id}`). Dipakai saat kasir
    /// menghapus bon yang pernah terkirim (bonId>0) supaya tak mengambang di web.
    /// 404 = bon uda dihapus dari web → anggap tak masalah.
    pub fn hapus_bon(&self, bon_id: i64) -> Result<(), String> {
        let resp = self.http_fast.delete(self.endpoint(&format!("/api/bon/{bon_id}")))
            .header("Cookie", self.auth_cookie())
            .send().map_err(|e| format!("network: {e}"))?;
        if resp.status().is_success() || resp.status().as_u16() == 404 {
            Ok(())
        } else {
            Err(format!("hapus bon {}: {}", bon_id, self.err_detail(resp)))
        }
    }
}
