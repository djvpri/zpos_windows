// ZPos Kasir — app desktop offline-first (Tauri v2).
// Rust backend: SQLite lokal (produk/member/antrian) + sinkronisasi ke server ZPos.
// Frontend: HTML/CSS/JS di webview, komunikasi via Tauri `invoke`.

mod db;
mod sync;

use db::AppDb;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use tauri::{Manager, State, WebviewWindow};

// ---------- state ----------
pub struct AppState {
    db: Mutex<Connection>,
    client: Mutex<Option<sync::SyncClient>>,
}

// ---------- tipe response utk frontend ----------
#[derive(Serialize)]
struct ProdukRow { id: i64, nama: String, harga: i64, stok: i64, kategori_id: Option<i64>, barcode: Option<String>, foto_url: Option<String>, k: Option<String> }

// ---------- commands ----------
#[tauri::command]
fn list_produk(state: State<AppState>) -> Result<Vec<ProdukRow>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut st = conn.prepare("SELECT p.id,p.nama,p.harga,p.stok,p.kategori_id,p.barcode,p.foto_url,k.nama FROM produk p LEFT JOIN kategori k ON k.id=p.kategori_id ORDER BY p.nama")
        .map_err(|e| e.to_string())?;
    let rows = st.query_map([], |r| Ok(ProdukRow{
        id: r.get(0)?, nama: r.get(1)?, harga: r.get(2)?, stok: r.get(3)?,
        kategori_id: r.get(4)?, barcode: r.get(5)?, foto_url: r.get(6)?, k: r.get(7)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<_,_>>().map_err(|e| e.to_string())
}

#[tauri::command]
fn cari_produk(state: State<AppState>, q: String) -> Result<Vec<ProdukRow>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let like = format!("%{}%", q);
    let mut st = conn.prepare("SELECT p.id,p.nama,p.harga,p.stok,p.kategori_id,p.barcode,p.foto_url,k.nama FROM produk p LEFT JOIN kategori k ON k.id=p.kategori_id WHERE p.nama LIKE ?1 OR p.barcode LIKE ?1 ORDER BY p.nama")
        .map_err(|e| e.to_string())?;
    let rows = st.query_map([&like], |r| Ok(ProdukRow{
        id: r.get(0)?, nama: r.get(1)?, harga: r.get(2)?, stok: r.get(3)?,
        kategori_id: r.get(4)?, barcode: r.get(5)?, foto_url: r.get(6)?, k: r.get(7)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<_,_>>().map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct AnggotaRow { id: i64, nama: String, telepon: Option<String>, kategori_member_id: Option<i64>, k: Option<String>, d: f64 }

#[tauri::command]
fn list_member(state: State<AppState>) -> Result<Vec<AnggotaRow>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut st = conn.prepare(
        "SELECT m.id, m.nama, m.telepon, m.kategori_member_id, km.nama, COALESCE(km.diskon_persen,0)
         FROM member m LEFT JOIN kategori_member km ON km.id=m.kategori_member_id ORDER BY m.nama"
    ).map_err(|e| e.to_string())?;
    let rows = st.query_map([], |r| Ok(AnggotaRow{
        id: r.get(0)?, nama: r.get(1)?, telepon: r.get(2)?, kategori_member_id: r.get(3)?,
        k: r.get(4)?, d: r.get(5)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<_,_>>().map_err(|e| e.to_string())
}

// Harga khusus member utk katalog: Map produk_id → harga efektif utk kategori member.
// Harga tetap menang; lalu diskon% kategori (negatif = markup); sisanya normal.
#[tauri::command]
fn harga_member(state: State<AppState>, member_id: i64) -> Result<serde_json::Map<String, Value>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let kat: Option<i64> = conn.query_row(
        "SELECT kategori_member_id FROM member WHERE id=?1", [member_id],
        |r| r.get(0),
    ).ok();
    let mut map = serde_json::Map::new();
    let Some(kat) = kat else { return Ok(map) };

    let diskon: f64 = conn.query_row(
        "SELECT diskon_persen FROM kategori_member WHERE id=?1", [kat],
        |r| r.get(0),
    ).unwrap_or(0.0);

    let mut st = conn.prepare(
        "SELECT p.id, p.harga, hm.harga FROM produk p
         LEFT JOIN harga_member hm ON hm.produk_id=p.id AND hm.kategori_member_id=?1"
    ).map_err(|e| e.to_string())?;
    let iter = st.query_map([kat], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i64>(1)?, r.get::<_,Option<i64>>(2)?)))
        .map_err(|e| e.to_string())?;
    for row in iter {
        let (pid, normal, tetap) = row.map_err(|e| e.to_string())?;
        let efektif = if let Some(t) = tetap { t }
        else if diskon != 0.0 { (normal as f64 * (1.0 - diskon/100.0)).round() as i64 }
        else { normal };
        map.insert(pid.to_string(), Value::from(efektif));
    }
    Ok(map)
}

#[derive(Deserialize)]
struct SimpanTrx {
    client_ref: String,
    payload: Value, // { trx, items } — format persis POST /api/transaksi server ZPos web
    #[serde(default)]
    metode: Option<String>,
    #[serde(default)]
    total: Option<i64>,
    #[serde(default)]
    user_id: Option<i64>,
    #[serde(default)]
    user_nama: Option<String>,
}

#[tauri::command]
fn antri_transaksi(state: State<AppState>, t: SimpanTrx) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let payload_json = serde_json::to_string(&t.payload).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO antrian (client_ref,produk,metode,total,dibuat_at,user_id,user_nama)
         VALUES (?1,?2,?3,?4,datetime('now'),?5,?6)",
        rusqlite::params![
            t.client_ref,
            payload_json, t.metode, t.total, t.user_id, t.user_nama
        ],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn jumlah_antrian(state: State<AppState>) -> Result<i64, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM antrian", [], |r| r.get(0)).unwrap_or(0);
    Ok(n)
}

// ---------- login PIN multiuser offline ----------
#[derive(Serialize)]
struct UserLokalRow {
    id: i64,
    nama: String,
    email: String,
    role: String,
    aktif: bool,
}

// Daftar user utk pengingat username di layar login. TAK sertakan pin_hash
// ke frontend — Rust yg verifikasi, JS cuma dapat pilihan nama.
#[tauri::command]
fn list_users(state: State<AppState>) -> Result<Vec<UserLokalRow>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut st = conn.prepare(
        "SELECT id, nama, email, role, aktif FROM users_lokal WHERE aktif=1 ORDER BY nama"
    ).map_err(|e| e.to_string())?;
    let rows = st.query_map([], |r| Ok(UserLokalRow{
        id: r.get(0)?, nama: r.get(1)?, email: r.get(2)?,
        role: r.get(3)?, aktif: r.get::<_,i64>(4)? != 0,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<_,_>>().map_err(|e| e.to_string())
}

// Verifikasi PIN 6 angka utk user_id, DIPERIKSA LOKAL terhadap pin_hash
// (bcrypt) di users_lokal. Offline & online sama. Attempt-lock: 5x gagal
// berturut-turut → kunci user tsb (perlu sync/online reset via lock reset).
#[tauri::command]
fn login_pin(state: State<AppState>, user_id: i64, pin: String) -> Result<bool, String> {
    // Validasi PIN bentuk: 6 digit angka (cegah payload ganjil).
    if pin.len() != 6 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("PIN harus 6 angka".into());
    }
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    // Attempt-lock: counter gagal disimpan di meta per user.
    let key = format!("pin_fail_{user_id}");
    let fail: i64 = conn.query_row(
        "SELECT COALESCE(v,0) FROM meta WHERE k=?1", [&key],
        |r| r.get(0),
    ).unwrap_or(0);
    if fail >= 5 {
        return Err("Terlalu banyak percobaan. Minta admin reset / sync ulang.".into());
    }

    let hash: Option<String> = conn.query_row(
        "SELECT pin_hash FROM users_lokal WHERE id=?1 AND aktif=1", [user_id],
        |r| r.get(0),
    ).ok();
    let Some(hash) = hash else {
        return Err("User tidak ditemukan atau nonaktif.".into());
    };
    if hash.is_empty() {
        return Err("User belum punya PIN. Minta admin set PIN.".into());
    }

    let ok = bcrypt::verify(&pin, &hash).map_err(|e| format!("verify: {e}"))?;
    if !ok {
        let n = fail + 1;
        conn.execute(
            "INSERT INTO meta (k,v) VALUES (?1,?2)
             ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            rusqlite::params![&key, n.to_string()],
        ).map_err(|e| e.to_string())?;
        if n >= 5 {
            return Err("PIN salah. Akun dikunci (5x gagal).".into());
        }
        return Err(format!("PIN salah ({n}x)."));
    }

    // Sukses → reset counter gagal.
    conn.execute("DELETE FROM meta WHERE k=?1", [&key]).map_err(|e| e.to_string())?;
    Ok(true)
}

// Panggil sinkron: tarik katalog+member, lalu kirim antrian. Butuh base_url + token.
#[tauri::command]
fn sync_remote(state: State<AppState>, app: tauri::AppHandle, base_url: String, token: String) -> Result<String, String> {
    // Command non-async → Tauri jalankan di worker thread. Guard Mutex boleh
    // ditahan (no await lintas), jadi tak ada masalah Send. Request pakai
    // reqwest::blocking (lihat Cargo.toml: fitur "blocking").
    // Jangan pernah tulis token asli ke log — cukup tandai ada/tidak.
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    // Prioritas token: meta `token_jwt` (hasil setup email+password yg pasti JWT valid).
    // Frontend auto-sync mungkin kirim token lama dr localStorage (mis. JWT_SECRET
    // yg salah), jadi token param HANYA fallback kalau meta kosong.
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let token = if !meta_tok.trim().is_empty() { meta_tok } else { token };
    submit_log(&app, &format!("sync mulai base={base_url} token={}", mask(token.as_str())));
    let c = sync::SyncClient::new(base_url.clone(), token);

    let r = (|| -> Result<(usize, usize, usize, usize, usize, usize, usize), String> {
        // `/api/auth/me` pertama: validasi token + dapat nama toko. Nama toko ini
        // dipakai deteksi GANTI TENANT — kalau beda dari sync sebelumnya, bersihkan
        // cache katalog/member lokal (produk/kategori upsert tak pernah hapus baris
        // dari toko lama, jadi kalau tak di-clear katalog tercampur antar tenant).
        let toko = c.pull_license(conn)?;  // Juga cache lisensi; return nama toko.
        let last: String = conn.query_row(
            "SELECT v FROM meta WHERE k='toko_terakhir'", [], |r| r.get::<_, String>(0),
        ).unwrap_or_default();
        // Kosong (belum tercatat / upgrade pertama) → anggap beda, bersihkan juga.
        // Pull isi-ulang penuh dari token valid, jadi hapus cache tak merugikan
        // (malah memastikan cache yg uda tercampur antar-tenant ikut dibersihkan).
        if last != toko {
            submit_log(&app, &format!("sync GANTI TENANT '{last}' -> '{toko}': bersihkan cache katalog/member"));
            for tbl in ["produk", "kategori", "member", "harga_member"] {
                conn.execute(&format!("DELETE FROM {tbl}"), []).map_err(|e| e.to_string())?;
            }
        }
        conn.execute(
            "INSERT INTO meta (k,v) VALUES ('toko_terakhir',?1) ON CONFLICT(k) DO UPDATE SET v=excluded.v",
            [&toko],
        ).map_err(|e| e.to_string())?;

        let n_kat = c.pull_kategori(conn)?;
        let n_produk = c.pull_produk(conn)?;
        let n_km = c.pull_kategori_member(conn)?;
        let n_member = c.pull_member(conn)?;
        // pull_users best-effort: cuma admin boleh (403 utk kasir). Kasir TETAP
        // bisa sinkron katalog/member; gagal tarik user TIDAK merusak sync.
        let n_user = match c.pull_users(conn) {
            Ok(n) => n,
            Err(e) => {
                submit_log(&app, &format!("sync pull_users SKIP: {e}"));
                0
            }
        };
        // pull_bon best-effort: tarik daftar bon gantung AKTIF dari web utk
        // ditampilkan/dilayani di kasir ini (merge by id di frontend). Gagal tidak
        // merusak sync (kasir tetap jalan dgn bon lokal).
        let n_bon = match c.pull_bon(conn) {
            Ok(n) => n,
            Err(e) => {
                submit_log(&app, &format!("sync pull_bon SKIP: {e}"));
                0
            }
        };
        let n_push = c.push_antrian(conn)?;
        Ok((n_kat, n_produk, n_km, n_member, n_user, n_bon, n_push))
    })();
    match &r {
        Ok((kk, pp, km, m, u, b, s)) => submit_log(&app, &format!("sync OK kategori={kk} produk={pp} katmember={km} member={m} user={u} bon={b} push={s}")),
        Err(e) => submit_log(&app, &format!("sync GAGAL: {e}")),
    }
    let (n_kat, n_produk, n_km, n_member, n_user, n_bon, n_push) = r?;
    Ok(format!("kategori {n_kat}, produk {n_produk}, kategori-member {n_km}, member {n_member}, user {n_user}, bon {n_bon}, push {n_push}"))
}

// Push antrian offline saja (tanpa tarik katalog/users). Dipakai siklus
// frontend pendek biar transaksi offline cepat sampai ke server, sedangkan
// tarik penuh (sync_remote) bisa dijalankan jarang — menahan db.lock jauh
// lebih singkat → UI tak blokir lama.
#[tauri::command]
fn push_antrian_only(state: State<AppState>, app: tauri::AppHandle, base_url: String, token: String) -> Result<usize, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let token = if !meta_tok.trim().is_empty() { meta_tok } else { token };
    let c = sync::SyncClient::new(base_url.clone(), token);
    let n = c.push_antrian(conn)?;
    submit_log(&app, &format!("push_antrian_only OK push={n}"));
    Ok(n)
}

// Setup pertama (owner/admin tenant): login email+password sekali → server
// validasi & balik daftar staff toko (auto-gen PIN utk yg belum punya).
// Tak perlu tempel JWT admin manual. Balik "(jumlah user, nama toko)".
#[tauri::command]
fn setup_kasir(state: State<AppState>, app: tauri::AppHandle, base_url: String, email: String, password: String) -> Result<String, String> {
    submit_log(&app, &format!("kasir-setup mulai base={base_url} email={email}"));
    let c = sync::SyncClient::new(base_url, String::new()); // token kosong — server validasi em+pass
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let r = c.setup_kasir(conn, &email, &password);
    match &r {
        Ok((n, toko)) => submit_log(&app, &format!("kasir-setup OK toko={toko} user={n}")),
        Err(e) => submit_log(&app, &format!("kasir-setup GAGAL: {e}")),
    }
    let (n, toko) = r?;
    Ok(format!("{} ({} kasir)", toko, n))
}

// Kasir mendaftarkan pelanggan baru (titik jual). Butuh online + token server;
// frontend mengecek status offline sebelum memanggil. Sukses → member masuk ke
// SQLite lokal dgn id server (konsisten dgn pull_member), balik nama.
#[tauri::command]
fn tambah_member(state: State<AppState>, app: tauri::AppHandle, base_url: String, nama: String, telepon: String, kategori_member_id: Option<i64>) -> Result<String, String> {
    let nama = nama.trim().to_string();
    if nama.is_empty() { return Err("Nama member wajib diisi".into()); }
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    let r = c.tambah_member(conn, &nama, &telepon, kategori_member_id);
    match &r {
        Ok(n) => submit_log(&app, &format!("member tambah OK nama={n} kat={:?}", kategori_member_id)),
        Err(e) => submit_log(&app, &format!("member tambah GAGAL: {e}")),
    }
    r
}

// Daftar kategori member toko (dropdown saat kasir daftarkan member).
#[tauri::command]
fn list_kategori_member(state: State<AppState>) -> Result<Vec<sync::RemoteKategoriMember>, String> {
    // Baca dari cache sqlite lokal (diisi `pull_kategori_member` saat sync).
    // Sebelumnya hit server live tiap dropdown → butuh online & lambat; kini
    // offline-ready & cepat. Server truth tetap lewat pull tiap siklus sync.
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut st = conn.prepare(
        "SELECT id,nama,diskon_persen FROM kategori_member ORDER BY urutan,nama",
    ).map_err(|e| e.to_string())?;
    let rows = st.query_map([], |r| Ok(sync::RemoteKategoriMember{
        id: r.get(0)?, nama: r.get(1)?, diskon_persen: r.get(2)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<_,_>>().map_err(|e| e.to_string())
}

// Info lisensi toko yg di-cache saat sync (`meta.lisensi`, diisi pull_license).
// Offline → baca cache lokal; frontend hitung sisa hari & tentukan blokir.
#[derive(Serialize, Deserialize)]
struct Lisensi {
    #[serde(default)]
    nama: String,
    #[serde(default)]
    alamat: String,
    #[serde(default)]
    telepon: String,
    #[serde(default)]
    catatan_struk: String,
    #[serde(default)]
    desain_nota: String,
    #[serde(default)]
    plan: String,
    #[serde(default)]
    aktif: bool,
    #[serde(default)]
    expired: bool,
    #[serde(default)]
    langganan_sampai: Option<String>,
}

#[tauri::command]
fn ambil_lisensi(state: State<AppState>) -> Result<Option<Lisensi>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let v: Option<String> = conn
        .query_row("SELECT v FROM meta WHERE k='lisensi'", [], |r| r.get(0))
        .ok();
    let Some(v) = v else { return Ok(None) };
    let l: Lisensi = serde_json::from_str(&v).map_err(|e| e.to_string())?;
    Ok(Some(l))
}

// Baca daftar bon gantung AKTIF yg ditarik web saat sync (meta `bon_sync`, raw
// JSON array). Frontend merge by id (skip-existing) → anti-duplikat & anti-overwrite.
#[tauri::command]
fn ambil_bon_sync(state: State<AppState>) -> Result<Option<String>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let v: Option<String> = conn
        .query_row("SELECT v FROM meta WHERE k='bon_sync'", [], |r| r.get(0))
        .ok();
    Ok(v)
}

// ===== Shift kasir (per kasir lokal = id web user) =====

// Buka shift utk kasir lokal. `base_url` + `user_id` datang dr frontend (kasir yg
// login pin). Server meng-assign shift ke user tsb (token sync = admin, admin-only).
#[tauri::command]
fn buka_shift(state: State<AppState>, app: tauri::AppHandle, base_url: String, user_id: i64, modal: i64) -> Result<sync::ShiftAktif, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    let r = c.buka_shift(conn, user_id, modal);
    match &r {
        Ok(s) => submit_log(&app, &format!("shift BUKA user={user_id} #{} modal={}", s.id, s.modal_awal)),
        Err(e) => submit_log(&app, &format!("shift BUKA GAGAL user={user_id}: {e}")),
    }
    r
}

// Tutup shift → server hitung rekap totals utk modal rekap.
#[tauri::command]
fn tutup_shift(state: State<AppState>, app: tauri::AppHandle, base_url: String, user_id: i64, shift_id: i64) -> Result<Option<sync::ShiftRekap>, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    let r = c.tutup_shift(conn, user_id, shift_id);
    match &r {
        Ok(_) => submit_log(&app, &format!("shift TUTUP user={user_id} #{}", shift_id)),
        Err(e) => submit_log(&app, &format!("shift TUTUP GAGAL user={user_id}: {e}")),
    }
    r
}

// Saldo kas live utk shift (modal + total_tunai − kas_keluar), dihitung server.
#[tauri::command]
fn saldo_shift(state: State<AppState>, base_url: String, shift_id: i64) -> Result<sync::SaldoShift, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    c.saldo_shift(shift_id)
}

// Saldo kas live utk shift offline (id negatif): hitung dari SQLite lokal, bukan server.
#[tauri::command]
fn saldo_shift_offline(state: State<AppState>, user_id: i64, shift_id: i64) -> Result<sync::SaldoShift, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(String::new(), meta_tok);
    c.saldo_shift_offline(conn, user_id, shift_id)
}

// Catat pengeluaran kas (kas keluar) utk shift aktif kasir → POST /api/kas-keluar.
#[tauri::command]
fn kirim_kas_keluar(state: State<AppState>, app: tauri::AppHandle, base_url: String, shift_id: i64, user_id: i64, kategori: String, nominal: i64, catatan: String) -> Result<i64, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    let r = c.kirim_kas_keluar(conn, shift_id, user_id, &kategori, nominal, &catatan);
    match &r {
        Ok(id) => submit_log(&app, &format!("kas keluar shift={shift_id} user={user_id} {kategori} Rp{nominal} → id {id}")),
        Err(e) => submit_log(&app, &format!("kas keluar GAGAL shift={shift_id}: {e}")),
    }
    r
}

// Daftar pengeluaran kas utk shift → GET /api/kas-keluar?shift_id=...
#[tauri::command]
fn daftar_kas_keluar(state: State<AppState>, base_url: String, shift_id: i64) -> Result<Vec<sync::KasKeluar>, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    c.daftar_kas_keluar(shift_id)
}

// Simpan bon gantung ke server → POST /api/bon. `produk` = {"<id>": qty} (JSON)
// hanya item asli (id>0); item virtual tidak bisa digantung ke web bon.
#[tauri::command]
fn kirim_bon(state: State<AppState>, app: tauri::AppHandle, base_url: String, nama: String, produk: String, total: i64) -> Result<i64, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    let r = c.kirim_bon(&nama, &produk, total);
    match &r {
        Ok(id) => submit_log(&app, &format!("bon gantung {nama} → id {id}")),
        Err(e) => submit_log(&app, &format!("bon gantung GAGAL ({nama}): {e}")),
    }
    r
}

// Tandai bon selesai (dibayar via windows) → PATCH /api/bon/{id}.
#[tauri::command]
fn tandai_bon(state: State<AppState>, app: tauri::AppHandle, base_url: String, bon_id: i64) -> Result<(), String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    let r = c.tandai_bon_selesai(bon_id);
    match &r {
        Ok(()) => submit_log(&app, &format!("bon #{bon_id} selesai")),
        Err(e) => submit_log(&app, &format!("bon #{bon_id} selesai GAGAL: {e}")),
    }
    r
}

// Hapus bon permanen di server (dipanggil saat kasir menghapus bon yang pernah
// terkirim, bonId>0) → DELETE /api/bon/{id}. Biar tak mengambang di tab Bon web.
#[tauri::command]
fn hapus_bon(state: State<AppState>, app: tauri::AppHandle, base_url: String, bon_id: i64) -> Result<(), String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    let meta_tok: String = conn.query_row(
        "SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0),
    ).unwrap_or_default();
    let c = sync::SyncClient::new(base_url, meta_tok);
    let r = c.hapus_bon(bon_id);
    match &r {
        Ok(()) => submit_log(&app, &format!("bon #{bon_id} dihapus dari server")),
        Err(e) => submit_log(&app, &format!("hapus bon #{bon_id} GAGAL: {e}")),
    }
    r
}

// Shift aktif kasir dari cache lokal (`meta.shift_{user_id}`, diisi buka_shift/cek_shift).
// Offline-safe; dipakai frontend utk banner + kirim shift_id di prosesBayar.
#[tauri::command]
fn ambil_shift(state: State<AppState>, user_id: i64, base_url: Option<String>) -> Result<Option<sync::ShiftAktif>, String> {
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;
    // Refresh dari server biar up-to-date dgn shift yg dibuka di web (admin).
    // Best-effort: offline/gagal → pakai cache lokal.
    if base_url.as_deref().map(|b| !b.trim().is_empty()).unwrap_or(false) {
        let meta_tok: String = conn
            .query_row("SELECT v FROM meta WHERE k='token_jwt'", [], |r| r.get::<_, String>(0))
            .unwrap_or_default();
        let c = sync::SyncClient::new(base_url.unwrap(), meta_tok);
        if let Ok(s) = c.cek_shift(conn, user_id) {
            return Ok(s);
        }
    }
    let v: Option<String> = conn
        .query_row("SELECT v FROM meta WHERE k=?1", [format!("shift_{user_id}")], |r| r.get(0))
        .ok();
    let Some(v) = v else { return Ok(None) };
    let s: sync::ShiftAktif = serde_json::from_str(&v).map_err(|e| e.to_string())?;
    Ok(Some(s))
}


// Jangan paparkan token penuh ke log — cukup "ada" (len>0) + 4 huruf terakhir.
fn mask(t: &str) -> String {
    if t.is_empty() { "KOSONG".into() }
    else if t.len() <= 6 { "****".into() }
    else { format!("...{} (len {})", &t[t.len()-4..], t.len()) }
}

// Buka DevTools (window webview) — utk diagnosa langsung dari UI.
// Dipanggil via tombol dikit di badge versi; hindari ketergantungan shortcut
// Ctrl+Shift+I yg di Tauri v2 kadang tak aktif meski devtools=true.
#[tauri::command]
fn buka_devtools(win: WebviewWindow) {
    let _ = win.open_devtools();
}

// Versi app dipakai frontend utk banding dgn release GitHub (tombol update).
#[tauri::command]
fn versi_app() -> String {
    env!("CARGO_PKG_VERSION").into()
}

// Buka tautan eksternal (mis. unduhan update) di browser default Windows.
// `window.open` di WebView2 Tauri sering diblokir/diam-saja — pakai `opener`
// biar URL benar2 terbuka di browser OS (garansi, bukan navigasi dalam webview).
#[tauri::command]
fn buka_url(url: String) -> Result<(), String> {
    opener::open(&url).map_err(|e| format!("gagal buka tautan: {e}"))
}

// ---------- updater in-app (portable, anti-SmartScreen) ----------
// Update diunduh dari DALAM app via reqwest (bukan browser). File yg ditulis
// stream bytes TIDAK membawa Zone.Identifier / Mark-of-the-Web → bila dijalankan
// lewat swap update, Windows TIDAK lagi menampilkan SmartScreen "aplikasi tidak
// dikenal" (yang muncul hanya utk file unduhan browser). Trojan fallback saat
// gagal swap: app lama tetap utuh, update.bin dibiarkan utk retry manual.

// Update via tauri-plugin-updater (NSIS installer): cek manifest `latest.json`
// di GitHub Release, unduh setup ter-sign, jalankan silent (autorestart).
// `ActionResult`: "PASANG" sukses (app restart dr dalam), error → frontend
// fallback ke jalur swap exe manual (`unduh_update`/`terapkan_update`).
#[tauri::command]
async fn apply_update(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| format!("init updater: {e}"))?;
    match updater.check().await {
        Ok(Some(update)) => {
            update
                .download_and_install(|_, _| {}, || {})
                .await
                .map_err(|e| format!("gagal pasang update: {e}"))?;
            // `install` (Windows) sudah ShellExecuteW setup.exe silent →
            // app restart sendiri via AUTOLAUNCHAPP. Tak ada way tp jaket lain.
            Ok("PASANG".into())
        }
        Ok(None) => Err("Sudah versi terbaru.".into()),
        Err(e) => Err(format!("cek update gagal: {e}")),
    }
}

// Unduh exe baru ke app_data mengikuti pola atomic (nama temp → rename).
// Retry 3x dgn client baru (koneksi segar). "error decoding response body"
// dari reqwest sering muncul saat pooled-connection stale / response besar
// terinterupsi; connect ulang menyelesaikannya. Timeout 120s biar tak hang.
#[tauri::command]
fn unduh_update(app: tauri::AppHandle, url: String) -> Result<String, String> {
    let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if std::fs::create_dir_all(&dir).is_err() { return Err("gagal akses app_data".into()); }
    let tmp = dir.join("zpos-kasir.new.tmp");
    let target = dir.join("zpos-kasir.new.exe");
    let mut last_err = String::from("tidak ada percobaan");
    for attempt in 0..3 {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build().map_err(|e| format!("build client gagal: {e}"))?;
        match client.get(&url).send() {
            Ok(resp) => {
                if !resp.status().is_success() {
                    last_err = format!("server balas HTTP {}", resp.status());
                } else {
                    match resp.bytes() {
                        Ok(bytes) => {
                            // Validasi exe: minimal 1MB + header `MZ` (PE). Kalau bukan exe
                            // yg sah, jangan ditimpa — updater tadi tak mengganti exe lama
                            // (& file kecil/rusak akan relaunch exe gagal → versi balik lama).
                            let ok_size = bytes.len() >= 1_000_000;
                            let ok_mz = bytes.len() >= 2 && bytes[0] == b'M' && bytes[1] == b'Z';
                            if ok_size && ok_mz {
                                if std::fs::write(&tmp, &bytes).is_err() { last_err = "tulis file gagal".into(); }
                                else if std::fs::rename(&tmp, &target).is_err() && !target.exists() {
                                    last_err = "atur ulang nama gagal".into();
                                } else {
                                    let _ = submit_log(&app, &format!("unduh OK {} byte", bytes.len()));
                                    return Ok(target.to_string_lossy().into());
                                }
                            } else {
                                last_err = format!("file unduhan bukan exe utuh ({} byte, mz={ok_mz})", bytes.len());
                            }
                        }
                        Err(e) => last_err = format!("baca response gagal: {e}"),
                    }
                }
            }
            Err(e) => last_err = format!("unduh gagal: {e}"),
        }
        if attempt < 2 {
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
    }
    Err(format!("unduh_gagal_3x: {last_err}"))
}

// Spawn cmd detach (2s delay biar app sempat exit) utk hapus exe lama → pindah
// exe baru → relaunch di tempat lama. Balik OK → frontend langsung keluar app.
// Arg diterima sbg Value & dibaca dua bentuk (targetPath / target_path,
// versiBaru / versi_baru) biar kompatibel caller lama (snake_case) & baru
// (camelCase) — Tauri 2 expose param snake_case sbg camelCase, exe tua masih
// kirim snake_case di invoke. ponytail: jika semua caller sudah camelCase,
// pindah ke param terketik.
#[tauri::command]
fn terapkan_update(app: tauri::AppHandle, payload: Value) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let p = payload.as_object().ok_or_else(|| format!(
        "Payload update salah (bukan object). Ini berarti app ini versi LAMA. Unduh manual exe dari halaman Release GitHub dan ganti file exe-nya. Versi sekarang: {}",
        env!("CARGO_PKG_VERSION")))?;
    let target_path = p.get("targetPath").or_else(|| p.get("target_path"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!(
            "Miss key targetPath. App ini versi LAMA (self-update dimulai v0.1.16). Unduh exe manual dari halaman Release GitHub, ganti file exe-nya, jalankan lagi. Versi sekarang: {}",
            env!("CARGO_PKG_VERSION")))?;
    let versi_baru = p.get("versiBaru").or_else(|| p.get("versi_baru"))
        .and_then(|v| v.as_str()).ok_or("missing required key versiBaru")?;
    let me = std::env::current_exe().map_err(|e| format!("path exe gagal: {e}"))?;
    let me_s = me.to_string_lossy().replace('/', "\\\\");
    let new_s = target_path.replace('/', "\\\\");
    let status_file = std::env::current_exe().ok()
        .and_then(|m| m.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("zpos-swap-status.txt");
    let status_s = status_file.to_string_lossy().replace('/', "\\\\");
    // Loop retry: exe lama TAK bisa dihapus selama app masih berjalan (terkunci
    // Windows). `timeout 2` kaku tak cukup — teardown Tauri bisa lebih lama.
    // Tunggu sampai `del` berhasil (app benar2 keluar) baru move + relaunch.
    // `echo result > status_file` dicatat utk diagnosa bila relaunch tak pernah
    // naik versi — perlu dibedakan "move ok" vs "move gagal".
    let script = format!(
        "timeout /t 1 /nobreak >nul\n\
:retry\n\
del /f /q {me} 2>nul\n\
if exist {me} ( timeout /t 1 /nobreak >nul & goto retry )\n\
move /y {new} {me} >nul\n\
if exist {me} (\n\
  echo moved-ok>{status}\n\
  start \"\" {me}\n\
) else (\n\
  echo move-FAIL>{status}\n\
)\n",
        me = format!("\"{}\"", me_s), new = format!("\"{}\"", new_s),
        status = format!("\"{}\"", status_s),
    );
    let _ = submit_log(&app, &format!("menerapkan update ke {}", versi_baru));
    // spawn tanpa .wait() → detach. Setelah app.exit(), cmd tunggu exe terlepas
    // (retry del) lalu pindah exe baru, relaunch di lokasi asal.
    std::process::Command::new("cmd.exe")
        .args(["/C", script.as_str()])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW — tanpa console
        .spawn()
        .map_err(|e| format!("spawn updater gagal: {e}"))?;
    Ok(())
}

#[tauri::command]
fn keluar(app: tauri::AppHandle) {
    app.exit(0);
}

// ---------- log error utk diagnosa ----------
// Frontend tulis error/time/pesan ke file teks di app_data_dir. Backend simpan
// path saat setup; `tulis_log` append satu baris, `baca_log` balik baris terakhir.
// `submit_log` = helper internal yg dipakai command backend (sync) dgn AppHandle,
// biar jejak error bisa direkam MESKI frontend/JS mati atau error terjadi di Rust.
// Konfig lokasi log: file `log_dir.txt` di app_data_dir berisi absolute path folder
// tujuan (kalau mau pindah dari default). Kalau file tak ada / folder tak valid →
// pakai app_data_dir (bawaan). Ditulis oleh command `pilih_log_dir`.
const LOG_DIR_FILE: &str = "log_dir.txt";

fn log_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let base = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let cfg = base.join(LOG_DIR_FILE);
    let dir = std::fs::read_to_string(&cfg)
        .ok()
        .map(|s| std::path::PathBuf::from(s.trim()))
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| base.clone());
    dir.join("zpos-errors.log")
}

// Buka dialog pilih folder (Windows native) utk lokasi simpan zpos-errors.log.
// Simpan pilihan ke log_dir.txt di app_data_dir. Balik path baru utk dipakai UI.
#[tauri::command]
fn pilih_log_dir(app: tauri::AppHandle) -> Result<String, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let picked = rfd::FileDialog::new()
        .set_title("Pilih lokasi simpan log error")
        .pick_folder()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if picked.is_empty() { return Ok(String::new()); } // user batal → biarkan
    let cfg = base.join(LOG_DIR_FILE);
    std::fs::write(&cfg, &picked).map_err(|e| format!("gagal simpan log dir: {e}"))?;
    Ok(picked)
}

// Path folder log yang sedang dipakai (utk tampil di input UI). Baca konfig,
// kalau tak ada → app_data_dir.
#[tauri::command]
fn get_log_dir(app: tauri::AppHandle) -> Result<String, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let cfg = base.join(LOG_DIR_FILE);
    Ok(std::fs::read_to_string(&cfg).map(|s| s.trim().to_string()).unwrap_or_else(|_| base.display().to_string()))
}

// Tulis satu baris ke zpos-errors.log (app append, timestamp readable).
// Fire-and-forget: gagal nulis TIDAK menggagalkan aksi utama.
fn submit_log(app: &tauri::AppHandle, msg: &str) {
    let p = log_path(app);
    let line = format!("{} | {}\n", chrono_now(), msg);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[tauri::command]
fn tulis_log(app: tauri::AppHandle, msg: String) -> Result<(), String> {
    let p = log_path(&app);
    let line = format!("{} | {}\n", chrono_now(), msg);
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
fn baca_log(app: tauri::AppHandle, tail: usize) -> Result<String, String> {
    let p = log_path(&app);
    let n = tail.max(1);
    if !p.exists() { return Ok(String::new()); }
    let txt = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = txt.lines().collect();
    Ok(lines.iter().rev().take(n).rev().cloned().collect::<Vec<_>>().join("\n"))
}

// Export zpos-errors.log ke file .txt di folder Downloads user. Kirim kembali
// path file yg dibuat (buat tampil/toast di frontend), atau Err kalau log kosong.
#[tauri::command]
fn export_log(app: tauri::AppHandle) -> Result<String, String> {
    let src = log_path(&app);
    let content = if src.exists() {
        std::fs::read_to_string(&src).unwrap_or_default()
    } else {
        String::new()
    };
    if content.trim().is_empty() {
        return Err("Log error masih kosong (belum ada aktivitas).".into());
    }
    let dir = app.path().download_dir().map_err(|e| format!("gagal dapat folder Downloads: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("gagal buat folder: {e}"))?;
    let fname = format!("zpos-errors-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let dst = dir.join(fname);
    std::fs::write(&dst, &content).map_err(|e| format!("gagal tulis file: {e}"))?;
    Ok(dst.to_string_lossy().into_owned())
}

// --- Cetak nota langsung ke printer thermal via Windows Print Spooler ----------
// Jalur Opsi A: kirim raw ESC/POS ke driver printer (RPP02N dll) tanpa buka
// browser / dialog print tiap cetak → jauh lebih cepat. Windows-only.

/// Daftar nama semua printer yang terpasang di Windows (EnumPrinters level 1).
#[cfg(windows)]
#[tauri::command]
fn daftar_printer() -> Result<Vec<String>, String> {
    use windows::core::PWSTR;
    use windows::Win32::Graphics::Printing::{EnumPrintersW, PRINTER_INFO_1W};

    // flags = PRINTER_ENUM_LOCAL (0x2) | PRINTER_ENUM_CONNECTIONS (0x4) biar USB
    // terpasang lokal + printer shared jaringan ikut ter-enumerasi (RPP02N via
    // USB muncul di spooler; printer BT virtual COM TIDAK—itu di luar spooler).
    const PRINTER_ENUM_LOCAL: u32 = 0x00000002;
    const PRINTER_ENUM_CONNECTIONS: u32 = 0x00000004;
    let flags: u32 = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed: u32 = 0;
    let mut returned: u32 = 0;
    // pass 1: hitung ukuran buffer. EnumPrintersW dgn buffer NULL (None) SELALU
    // return FALSE + ERROR_INSUFFICIENT_BUFFER (0x8007007A) utk query ukuran—ini
    // PERILAKU YANG DIHARAPKAN dua-pass, BUKAN kegagalan. Jangan `?` di sini
    // (pakai 0x8007007A, seen user melapor: 'Gagal enumerasi printer'). Cukup
    // baca `needed` hasil; kalau 0 berarti tak ada printer lokal.
    let _hr = unsafe { EnumPrintersW(flags, None, 1, None, &mut needed, &mut returned) };
    if needed == 0 {
        return Ok(Vec::new()); // tak ada printer lokal
    }
    let mut buf = vec![0u8; needed as usize];
    let mut returned2: u32 = 0;
    unsafe {
        EnumPrintersW(
            flags,
            None,
            1,
            Some(&mut buf[..]),
            &mut needed,
            &mut returned2,
        )
    }
    .map_err(|e| format!("EnumPrintersW (isi) gagal: {e}"))?;
    // PRINTER_INFO_1W { flags:u32, pDescription, pName, pComment }
    let stride = std::mem::size_of::<PRINTER_INFO_1W>();
    let n = returned2 as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let base = buf.as_ptr() as usize + i * stride;
        let info = unsafe { &*(base as *const PRINTER_INFO_1W) };
        unsafe {
            if !info.pName.is_null() {
                let name = info.pName.to_string().map_err(|e| format!("baca nama printer: {e}"))?;
                out.push(name);
            }
        }
    }
    Ok(out)
}

#[cfg(not(windows))]
#[tauri::command]
fn daftar_printer() -> Result<Vec<String>, String> {
    Err("Cetak via spooler hanya didukung di Windows.".into())
}

/// Kirim raw ESC/POS ke printer bernama `nama_printer`. String ESC/POS dihasilkan
/// frontend (buildEscPos). Windows-only via OpenPrinter/StartDoc/WritePrinter.
#[cfg(windows)]
#[tauri::command]
fn cetak_escpos(escpos: String, nama_printer: String) -> Result<String, String> {
    use std::os::raw::c_void;
    use windows::core::{w, PCWSTR, PWSTR};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Printing::{
        ClosePrinter, EndDocPrinter, OpenPrinterW, StartDocPrinterW, WritePrinter, DOC_INFO_1W,
    };

    if nama_printer.trim().is_empty() {
        return Err("Nama printer kosong.".into());
    }
    // encode nama → utf-16 null-terminated
    let name16: Vec<u16> = nama_printer.encode_utf16().chain(Some(0)).collect();
    let pname = PCWSTR(name16.as_ptr());

    let mut hprinter: HANDLE = HANDLE::default();
    unsafe { OpenPrinterW(pname, &mut hprinter, None) }
        .map_err(|_| {
            format!(
                "Printer \"{nama_printer}\" tidak bisa dibuka. Pastikan driver RPP02N terpasang di Printers & scanners."
            )
        })?;
    // cegah leak & jangan pernah lupa close printer pada error
    let r = (|| -> Result<String, String> {
        let doc = DOC_INFO_1W {
            pDocName: PWSTR(w!("Z1 Kasir nota").as_ptr() as *mut u16),
            pOutputFile: PWSTR::null(),
            pDatatype: PWSTR(w!("RAW").as_ptr() as *mut u16),
        };
        let job: u32 = unsafe { StartDocPrinterW(hprinter, 1, &doc) };
        if job == 0 {
            return Err("StartDocPrinter gagal.".into());
        }
        let data = escpos.as_bytes();
        let mut written: u32 = 0;
        let okw: windows::Win32::Foundation::BOOL = unsafe {
            WritePrinter(hprinter, data.as_ptr() as *const c_void, data.len() as u32, &mut written)
        };
        let _ = unsafe { EndDocPrinter(hprinter) };
        if !okw.as_bool() {
            return Err("WritePrinter gagal mengirim ESC/POS.".into());
        }
        Ok(format!("Terkirim {} byte ke {nama_printer}.", written))
    })();
    let _ = unsafe { EndDocPrinter(hprinter) };
    let _ = unsafe { ClosePrinter(hprinter) };
    r
}

/// Buka laci kasir (cash drawer) via ESC/POS `ESC p` ke printer nama_printer.
/// Data pulsa: 1B 70 00 19 19 = pin 2, on 50ms, off 50ms (umum utk kasir thermal).
/// Reuse jalur raw spooler yg sama dgn cetak_escpos (OpenPrinter/WritePrinter).
#[cfg(windows)]
#[tauri::command]
fn buka_laci(app: tauri::AppHandle, nama_printer: String) -> Result<String, String> {
    use std::os::raw::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Printing::{
        ClosePrinter, EndDocPrinter, OpenPrinterW, StartDocPrinterW, WritePrinter, DOC_INFO_1W,
    };
    use windows::core::PWSTR;
    use windows::core::w;
    if nama_printer.trim().is_empty() {
        return Err("Nama printer kosong.".into());
    }
    // Jejak buka laci di zpos-errors.log — bedain manual (tombol) vs auto (ssdh nota):
    // dua-duanya panggil buka_laci, tapi kalau laci diam, log ini tunjuk di mana gagalnya.
    submit_log(&app, &format!("BUKA_LACI_TRY printer={nama_printer}"));
    let name16: Vec<u16> = nama_printer.encode_utf16().chain(Some(0)).collect();
    let pname = PCWSTR(name16.as_ptr());
    let mut hprinter: HANDLE = HANDLE::default();
    if let Err(e) = unsafe { OpenPrinterW(pname, &mut hprinter, None) } {
        let msg = format!("Printer \"{nama_printer}\" tidak bisa dibuka — pastikan driver terpasang di Printers & scanners. ({e})");
        submit_log(&app, &format!("BUKA_LACI_FAIL open: {msg}"));
        return Err(msg);
    }
    let r = (|| -> Result<String, String> {
        let doc = DOC_INFO_1W {
            pDocName: PWSTR(w!("ZPos laci").as_ptr() as *mut u16),
            pOutputFile: PWSTR::null(),
            pDatatype: PWSTR(w!("RAW").as_ptr() as *mut u16),
        };
        let job: u32 = unsafe { StartDocPrinterW(hprinter, 1, &doc) };
        if job == 0 {
            return Err("StartDocPrinter gagal.".into());
        }
        let data: &[u8] = &[0x1B, 0x70, 0x00, 0x19, 0x19]; // ESC p 0 25 25
        let mut written: u32 = 0;
        let okw: windows::Win32::Foundation::BOOL = unsafe {
            WritePrinter(hprinter, data.as_ptr() as *const c_void, data.len() as u32, &mut written)
        };
        let _ = unsafe { EndDocPrinter(hprinter) };
        if !okw.as_bool() {
            return Err("WritePrinter gagal kirim pulsa laci.".into());
        }
        Ok(format!("Laci dibuka lewat {nama_printer}."))
    })();
    let _ = unsafe { EndDocPrinter(hprinter) };
    let _ = unsafe { ClosePrinter(hprinter) };
    match &r {
        Ok(m) => submit_log(&app, &format!("BUKA_LACI_OK {m}")),
        Err(e) => submit_log(&app, &format!("BUKA_LACI_FAIL {e}")),
    }
    r
}

#[cfg(not(windows))]
#[tauri::command]
fn buka_laci(app: tauri::AppHandle, nama_printer: String) -> Result<String, String> {
    let _ = (app, nama_printer);
    Err("Buka laci hanya didukung di Windows.".into())
}

#[cfg(not(windows))]
#[tauri::command]
fn cetak_escpos(escpos: String, nama_printer: String) -> Result<String, String> {
    Err("Cetak via spooler hanya didukung di Windows.".into())
}

// Tulis HTML nota transaksi ke file sementara (temp_dir/zpos-nota/). Frontend
// lalu panggil `buka_url` → opener buka file itu di browser default, biar `print()`
// jalan (WebView2 blokir window.open). Nama file unik per detik→hindari tabrakan.
#[tauri::command]
fn nota_temp(html: String) -> Result<String, String> {
    let dir = std::env::temp_dir().join("zpos-nota");
    std::fs::create_dir_all(&dir).map_err(|e| format!("gagal buat folder nota: {e}"))?;
    let f = dir.join(format!("nota-{}.html", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")));
    std::fs::write(&f, html).map_err(|e| format!("gagal tulis nota: {e}"))?;
    Ok(f.to_string_lossy().into_owned())
}

// Timestamp readable lokal. chrono fitur `clock` (default) dipakai — bukan
// SystemTime/epoch, supaya zpos-errors.log gampang diurut & dibaca.
fn chrono_now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Hanya satu app ZPos Kasir boleh jalan per perangkat. Peluncuran
            // kedua → fokus ke window yang sudah terbuka, lalu tutup proses baru.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
                let _ = w.show();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            if std::fs::create_dir_all(&dir).is_err() {
                // Kalau gagal bikin dir, log tetap dicoba (fallback ke ".").
                eprintln!("ZPos: gagal create dir {}", dir.display());
            }
            let path = dir.join("zpos.db");
            // Jejak boot biar dari zpos-errors.log langsung tau build mana & apakah
            // startup sampai ke sini (kalau app tak muncul, cek baris START ini).
            let handle = app.handle();
            submit_log(handle, &format!(
                "START versi={} data_dir={}", env!("CARGO_PKG_VERSION"), dir.display()
            ));
            // Jejak FORENSIK exe yg berjalan: path absolut + ukuran + timestamp file.
            // Dipakai utk diagnosa "update tak naik versi" — bandingkan dgn asset rilis.
            // Kalau product version file ≠ versi string, atau path di luar folder instal,
            // itu bukti exe yg dijalankan bukan yg di-install.
            if let Ok(me) = std::env::current_exe() {
                let meta = std::fs::metadata(&me).ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let mtime = meta.as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        use std::time::UNIX_EPOCH;
                        t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
                    }).unwrap_or(0);
                submit_log(handle, &format!(
                    "EXE path={} size={} mtime={}", me.display(), size, mtime
                ));
            }
            let conn = match Connection::open(&path) {
                Ok(c) => { submit_log(handle, "DB open OK"); c }
                Err(e) => {
                    submit_log(handle, &format!("DB open GAGAL: {e}"));
                    panic!("buka db: {e}");
                }
            };
            match db::init(&conn) {
                Ok(()) => submit_log(handle, "DB init OK"),
                Err(e) => {
                    submit_log(handle, &format!("DB init GAGAL: {e}"));
                    panic!("init db: {e}");
                }
            }
            app.manage(AppState { db: Mutex::new(conn), client: Mutex::new(None) });
            submit_log(handle, "setup selesai, window akan tampil");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_produk, cari_produk, list_member, harga_member,
            antri_transaksi, jumlah_antrian, sync_remote, push_antrian_only, buka_devtools,
            list_users, login_pin,
            setup_kasir, tambah_member, list_kategori_member,
            versi_app,
            buka_url,
            unduh_update, terapkan_update, apply_update, keluar,
            tulis_log, baca_log, export_log, pilih_log_dir, get_log_dir, nota_temp,
            daftar_printer, cetak_escpos, buka_laci, ambil_lisensi, ambil_bon_sync,
            buka_shift, tutup_shift, ambil_shift,
            saldo_shift, saldo_shift_offline, kirim_kas_keluar, daftar_kas_keluar,
            kirim_bon, tandai_bon, hapus_bon
        ])
        .run(tauri::generate_context!())
        .expect("gagal menjalankan ZPos Kasir");
}

// Entrypoint dipanggil dari src/main.rs (binary). Dipisah biar lib.rs dites.
pub fn run_app() {
    run();
}
