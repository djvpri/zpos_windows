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
use tauri::{Manager, State};

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
    produk: Vec<Value>, // [{id, qty, harga}]
    metode: String,
    total: i64,
}

#[tauri::command]
fn antri_transaksi(state: State<AppState>, t: SimpanTrx) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO antrian (client_ref,produk,metode,total,dibuat_at) VALUES (?1,?2,?3,?4,datetime('now'))",
        rusqlite::params![t.client_ref, serde_json::to_string(&t.produk).map_err(|e| e.to_string())?, t.metode, t.total],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn jumlah_antrian(state: State<AppState>) -> Result<i64, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM antrian", [], |r| r.get(0)).unwrap_or(0);
    Ok(n)
}

// Panggil sinkron: tarik katalog+member, lalu kirim antrian. Butuh base_url + token.
#[tauri::command]
fn sync_remote(state: State<AppState>, base_url: String, token: String) -> Result<String, String> {
    // Command non-async → Tauri jalankan di worker thread. Guard Mutex boleh
    // ditahan (no await lintas), jadi tak ada masalah Send. Request pakai
    // reqwest::blocking (lihat Cargo.toml: fitur "blocking").
    let c = sync::SyncClient::new(base_url, token);
    let mut guard = state.db.lock().map_err(|e| e.to_string())?;
    let conn = &mut *guard;

    let n_kat = c.pull_kategori(conn)?;
    let n_produk = c.pull_produk(conn)?;
    let n_member = c.pull_member(conn)?;
    let n_push = c.push_antrian(conn)?;
    Ok(format!("kategori {n_kat}, produk {n_produk}, member {n_member}, push {n_push}"))
}

// Buka DevTools (window webview) — utk diagnosa langsung dari UI.
// Dipanggil via tombol dikit di badge versi; hindari ketergantungan shortcut
// Ctrl+Shift+I yg di Tauri v2 kadang tak aktif meski devtools=true.
#[tauri::command]
fn buka_devtools(window: tauri::Window) {
    let _ = window.open_devtools();
}

fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            std::fs::create_dir_all(&dir).ok();
            let path = dir.join("zpos.db");
            let conn = Connection::open(&path).expect("buka db");
            db::init(&conn).expect("init db");
            app.manage(AppState { db: Mutex::new(conn), client: Mutex::new(None) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_produk, cari_produk, list_member, harga_member,
            antri_transaksi, jumlah_antrian, sync_remote, buka_devtools
        ])
        .run(tauri::generate_context!())
        .expect("gagal menjalankan ZPos Kasir");
}

// Entrypoint dipanggil dari src/main.rs (binary). Dipisah biar lib.rs dites.
pub fn run_app() {
    run();
}
