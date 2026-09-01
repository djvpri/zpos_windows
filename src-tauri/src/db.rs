use rusqlite::{Connection, Result};
use std::sync::Mutex;

pub struct AppDb(pub Mutex<Connection>);

// Skema lokal: seluruh data operasional kasir disimpan offline di SQLite.
// Produk & member di-"-sync" dari server ZPos saat online; transaksi diantri
// lalu dipush ke server saat koneksi kembali.
pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;

        CREATE TABLE IF NOT EXISTS produk (
            id          INTEGER PRIMARY KEY,
            nama        TEXT NOT NULL,
            harga       INTEGER NOT NULL,      -- harga ecer normal
            stok        INTEGER NOT NULL DEFAULT 0,
            kategori_id INTEGER,
            barcode     TEXT,
            barcode_internal TEXT,
            foto_url    TEXT,
            jenis       TEXT NOT NULL DEFAULT 'fisik',    -- 'fisik' | 'digital'
            buyer_sku_code TEXT,                -- kode Digiflazz (xld10, pln, ...)
            digital_brand TEXT NOT NULL DEFAULT 'prabayar', -- 'prabayar' | 'pasca'
            updated_at  TEXT
        );

        CREATE TABLE IF NOT EXISTS kategori (
            id   INTEGER PRIMARY KEY,
            nama TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS kategori_member (
            id            INTEGER PRIMARY KEY,
            nama          TEXT NOT NULL,
            diskon_persen REAL NOT NULL DEFAULT 0,  -- negatif = markup
            urutan        INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS member (
            id                 INTEGER PRIMARY KEY,
            nama               TEXT NOT NULL,
            telepon            TEXT,
            kategori_member_id INTEGER
        );

        CREATE TABLE IF NOT EXISTS harga_member (
            produk_id          INTEGER NOT NULL,
            kategori_member_id INTEGER NOT NULL,
            harga              INTEGER,
            PRIMARY KEY (produk_id, kategori_member_id)
        );

        -- Antrian transaksi offline. client_ref mencegah duplikat saat push.
        -- user_id/user_nama: tandai siapa kasir (multiuser 1 perangkat).
        CREATE TABLE IF NOT EXISTS antrian (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            client_ref TEXT NOT NULL,
            produk     TEXT NOT NULL,   -- JSON [{id, qty, harga}]
            metode     TEXT NOT NULL,
            total      INTEGER NOT NULL,
            dibuat_at  TEXT NOT NULL,
            user_id    INTEGER,
            user_nama  TEXT
        );

        -- Daftar user toko yang disinkron dari server (utk login PIN offline).
        -- pin_hash = bcrypt user.kasir_pin_hash dari server.
        CREATE TABLE IF NOT EXISTS users_lokal (
            id       INTEGER PRIMARY KEY,
            toko_id  INTEGER NOT NULL,
            nama     TEXT NOT NULL,
            email    TEXT NOT NULL,
            role     TEXT NOT NULL,
            aktif    INTEGER NOT NULL DEFAULT 1,
            pin_hash TEXT
        );

        CREATE TABLE IF NOT EXISTS meta (
            k TEXT PRIMARY KEY,
            v TEXT
        );
        "#,
    )?;

    // Migration idempoten: DB lama (antrian tanpa user_id/user_nama) pakai ALTER.
    let cols_exist: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('antrian')")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>>>()?;
    let has_uid = cols_exist.iter().any(|c| c == "user_id");
    if !has_uid {
        conn.execute_batch(
            "ALTER TABLE antrian ADD COLUMN user_id INTEGER;
             ALTER TABLE antrian ADD COLUMN user_nama TEXT;",
        )?;
    }

    // Migration produk: DB lama tanpa `barcode_internal` (a.k.a kolom baru utk
    // barcode pendek label 25mm) → tambah via ALTER supaya sync isi kolomnya.
    let pcols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('produk')")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<_>>>()?;
    if !pcols.iter().any(|c| c == "barcode_internal") {
        conn.execute("ALTER TABLE produk ADD COLUMN barcode_internal TEXT", [])?;
    }
    // Migration produk DIGITAL: DB lama tanpa kolom jenis/buyer_sku_code/digital_brand
    // → tambah via ALTER. Produk lama default 'fisik' (perilaku lama terjaga).
    if !pcols.iter().any(|c| c == "jenis") {
        conn.execute("ALTER TABLE produk ADD COLUMN jenis TEXT NOT NULL DEFAULT 'fisik'", [])?;
    }
    if !pcols.iter().any(|c| c == "buyer_sku_code") {
        conn.execute("ALTER TABLE produk ADD COLUMN buyer_sku_code TEXT", [])?;
    }
    if !pcols.iter().any(|c| c == "digital_brand") {
        conn.execute("ALTER TABLE produk ADD COLUMN digital_brand TEXT NOT NULL DEFAULT 'prabayar'", [])?;
    }

    Ok(())
}
