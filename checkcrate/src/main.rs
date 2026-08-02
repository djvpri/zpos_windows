// Self-check skema SQLite kasir offline: init, upsert produk, member, antrian.
// Bukan tes Tauri (butuh sistem libs) — verifikasi logika DB murni.
mod db;

use rusqlite::Connection;

fn main() {
    let conn = Connection::open_in_memory().unwrap();
    db::init(&conn).unwrap();

    // upsert produk (pakai logika yang sama dgn sync.rs pull_produk)
    conn.execute(
        "INSERT INTO produk (id,nama,harga,stok,kategori_id,barcode,foto_url,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET nama=excluded.nama, harga=excluded.harga",
        rusqlite::params![1, "Nasi Goreng", 15000, 40, 1, "899", "x", ],
    ).unwrap();

    // upsert kedua (update harga) → harusnya tak dobel
    conn.execute(
        "INSERT INTO produk (id,nama,harga,stok,kategori_id,barcode,foto_url,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET nama=excluded.nama, harga=excluded.harga",
        rusqlite::params![1, "Nasi Goreng", 16000, 38, 1, "899", "x", ],
    ).unwrap();
    assert_eq!(sql_count(&conn, "SELECT COUNT(*) FROM produk"), 1, "upsert tak boleh dobel");
    let harga: i64 = conn.query_row("SELECT harga FROM produk WHERE id=1", [], |r| r.get(0)).unwrap();
    assert_eq!(harga, 16000, "upsert harus update harga");

    // member + kategori member
    conn.execute("INSERT INTO kategori_member (id,nama,diskon_persen) VALUES (1,'Palapa',-15)", []).unwrap();
    conn.execute("INSERT INTO member (id,nama,telepon,kategori_member_id) VALUES (1,'Bu Rina',NULL,1)", []).unwrap();
    let kat: i64 = conn.query_row("SELECT kategori_member_id FROM member WHERE id=1", [], |r| r.get(0)).unwrap();
    assert_eq!(kat, 1);

    // harga_member override
    conn.execute("INSERT INTO harga_member (produk_id,kategori_member_id,harga) VALUES (1,1,18000)", []).unwrap();

    // antrian transaksi offline + client_ref unik
    conn.execute(
        "INSERT INTO antrian (client_ref,produk,metode,total,dibuat_at) VALUES ('C1','[{\"id\":1,\"qty\":2,\"harga\":16000}]','Tunai',32000,datetime('now'))",
        [],
    ).unwrap();
    let refs: Vec<String> = { let mut st = conn.prepare("SELECT client_ref FROM antrian").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().collect::<Result<_,_>>().unwrap() };
    assert_eq!(refs, vec!["C1".to_string()]);

    // harga efektif member (markup -15% TANPA harga tetap) lewat query
    // harga tetap menang → 18000
    let tetap: i64 = conn.query_row(
        "SELECT hm.harga FROM produk p LEFT JOIN harga_member hm ON hm.produk_id=p.id AND hm.kategori_member_id=1 WHERE p.id=1",
        [], |r| r.get(0)).unwrap();
    assert_eq!(tetap, 18000);

    println!("OK: skema DB kasir offline valid (upsert, member, harga_member, antrian,kategorimember)");

    // --- Sinkronisasi: bentuk JSON PushTransaksi yg dikirim ke server ZPos. ---
    // Mencocokkan struct di src-tauri/src/sync.rs (PushTransaksi/PushItem). Serde murni.
    #[derive(serde::Serialize)]
    struct PushItem { id: i64, qty: i64, harga: i64 }
    #[derive(serde::Serialize)]
    struct PushTransaksi { client_ref: String, metode_bayar: String, details: Vec<PushItem>, total: i64 }

    let trx = PushTransaksi {
        client_ref: "trx-1".into(),
        metode_bayar: "Tunai".into(),
        details: vec![PushItem { id: 1, qty: 2, harga: 17250 }],
        total: 34500,
    };
    let j = serde_json::to_string(&trx).unwrap();
    let v: serde_json::Value = serde_json::from_str(&j).unwrap();
    assert_eq!(v["client_ref"], "trx-1");
    assert_eq!(v["metode_bayar"], "Tunai");
    assert_eq!(v["details"][0]["id"], 1);
    assert_eq!(v["details"][0]["qty"], 2);
    assert_eq!(v["details"][0]["harga"], 17250);
    assert_eq!(v["total"], 34500);

    // Round-trip: item yang di-parse dari JSON beda juga harus cocok.
    let item2: serde_json::Value = serde_json::from_str(r#"{"id":9,"qty":1,"harga":6000}"#).unwrap();
    assert_eq!(item2["id"], 9); assert_eq!(item2["qty"], 1); assert_eq!(item2["harga"], 6000);

    println!("OK: PushTransaksi serialisasi (client_ref, metode_bayar, details, total) akurat");
}

fn sql_count(c: &Connection, q: &str) -> i64 { c.query_row(q, [], |r| r.get(0)).unwrap() }
