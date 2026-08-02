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
            foto_url    TEXT,
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
        CREATE TABLE IF NOT EXISTS antrian (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            client_ref TEXT NOT NULL,
            produk     TEXT NOT NULL,   -- JSON [{id, qty, harga}]
            metode     TEXT NOT NULL,
            total      INTEGER NOT NULL,
            dibuat_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS meta (
            k TEXT PRIMARY KEY,
            v TEXT
        );
        "#,
    )?;
    Ok(())
}
