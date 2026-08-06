// fixtures/db.mjs
// In-memory SQLite (node:sqlite) yang meniru skema backend Rust (src-tauri/src/db.rs)
// persis — supaya kontrak kolom/query antara frontend JS dan backend bisa diverifikasi
// deterministik TANPA butuh binary Rust/build. Seed data test disediakan.
//
// Mengapa: harness Playwright me-mock `__TAURI__` invoke; untuk kontrak data, mock
// mejawab dari SQLite nyata (bukan objek hardcode) supaya assertion melawan skema db.rs.

import { DatabaseSync } from 'node:sqlite';
import { mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));

// Skema — dikopi VERBATIM dari db.rs `init()` biar melawan drift.
export const SKEMA = `
CREATE TABLE IF NOT EXISTS produk (
    id          INTEGER PRIMARY KEY,
    nama        TEXT NOT NULL,
    harga       INTEGER NOT NULL,
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
    diskon_persen REAL NOT NULL DEFAULT 0,
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
CREATE TABLE IF NOT EXISTS antrian (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    client_ref TEXT NOT NULL,
    produk     TEXT NOT NULL,
    metode     TEXT NOT NULL,
    total      INTEGER NOT NULL,
    dibuat_at  TEXT NOT NULL,
    user_id    INTEGER,
    user_nama  TEXT
);
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
`;

// bcrypt hash untuk PIN "123456" (login_pin pakai bcrypt::verify di Rust). Hash real.
// Hash ini untuk string "123456" — berguna utk menguji kontrak login PIN offline.
export const PIN_BCRYPT = '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxZP0l4hxVfLqIqzkR5e7Y9wE0G';

function seed(db) {
  // skema kosong — utk assertion "schema ada" (di timpa seed di bawah utk data test)
}

// Seed data test — kecil tapi skala nyata (cakup markup, diskon, harga tetap).
export function seedTestData(db) {
  db.exec(`
    DELETE FROM produk; DELETE FROM kategori; DELETE FROM kategori_member;
    DELETE FROM member; DELETE FROM harga_member; DELETE FROM antrian;
    DELETE FROM users_lokal; DELETE FROM meta;

    INSERT INTO kategori (id,nama) VALUES (1,'Makanan'),(2,'Minuman');

    INSERT INTO produk (id,nama,harga,stok,kategori_id,barcode) VALUES
      (1,'Nasi Goreng',15000,40,1,'8991001'),
      (2,'Es Teh',5000,120,2,'8991002'),
      (3,'Kopi Susu',12000,15,2,'8991003'),
      (4,'Roti Bakar',9000,12,1,NULL);

    -- kategori_member: id 1 = markup -7% (kasus fix 0.1.17 de_f64_str), id2 = diskon 10%
    INSERT INTO kategori_member (id,nama,diskon_persen,urutan) VALUES
      (1,'Bon Pribadi',-7,1),
      (2,'Loyal',10,2);

    INSERT INTO member (id,nama,telepon,kategori_member_id) VALUES
      (1,'Syahrul','081',1),
      (2,'Bu Rina','082',2);

    -- harga tetap: produk 2 utk member kat 1 = 4500
    INSERT INTO harga_member (produk_id,kategori_member_id,harga) VALUES (2,1,4500);

    -- users_lokal: kasir + admin (pin_hash diset oleh caller; di sini pbkdf2 dummy utk
    -- login_pin test dgn verify() yang TIDAK bcrypt — lihat mock).
    INSERT INTO users_lokal (id,toko_id,nama,email,role,aktif,pin_hash) VALUES
      (1,1,'Kasir A','kasir@z','kasir',1,''),
      (2,1,'Admin','admin@z','admin',1,'');
  `);
}

// Ciptakan (atau baca) DB di fixtures dir. Murni data test, tak pernah menyentuh
// data_dir produksi user (%APPDATA%).
export function createDb({ seed: doSeed = true } = {}) {
  const fdir = join(ROOT, '.fixtures');
  mkdirSync(fdir, { recursive: true });
  const db = new DatabaseSync(join(fdir, 'zpos.db'));
  db.exec(SKEMA);
  if (doSeed) seedTestData(db);
  return db;
}

// --- query layer dipakai mock invoke ---
// mimic list_produk query db.rs (kategori nama via LEFT JOIN k).
export function qListProduk(db) {
  return db.prepare(`SELECT p.id,p.nama,p.harga,p.stok,p.kategori_id,p.barcode,p.foto_url,k.nama AS kn
    FROM produk p LEFT JOIN kategori k ON k.id=p.kategori_id ORDER BY p.nama`).all()
    .map(r => ({ id: r.id, nama: r.nama, harga: r.harga, stok: r.stok, kategori_id: r.kategori_id, barcode: r.barcode, foto_url: r.foto_url, k: r.kn }));
}

export function qListMember(db) {
  return db.prepare(`SELECT m.id, m.nama, m.telepon, m.kategori_member_id, km.nama AS kn, COALESCE(km.diskon_persen,0) AS d
    FROM member m LEFT JOIN kategori_member km ON km.id=m.kategori_member_id ORDER BY m.nama`).all()
    .map(r => ({ id: r.id, nama: r.nama, telepon: r.telepon, kategori_member_id: r.kategori_member_id, k: r.kn, d: r.d }));
}

export function qListKategoriMember(db) {
  return db.prepare(`SELECT id,nama,diskon_persen FROM kategori_member ORDER BY urutan,id`).all()
    .map(r => ({ id: r.id, nama: r.nama, diskon_persen: r.diskon_persen }));
}

export function qListUsers(db) {
  return db.prepare(`SELECT id, nama, email, role, aktif FROM users_lokal WHERE aktif=1 ORDER BY nama`).all()
    .map(r => ({ id: r.id, nama: r.nama, email: r.email, role: r.role, aktif: !!r.aktif }));
}

// --- disk-override path ---
// Ciptakan DB fixture sebagai FILE (utk assertion baca-disk + schema drift check).
// Bisa di-snapshot/diff antar run.
export function createDbFile() {
  const fdir = join(ROOT, '.fixtures');
  mkdirSync(fdir, { recursive: true });
  const path = join(fdir, 'zpos-fixture.db');
  rmSync(path, { force: true });
  const db = new DatabaseSync(path);
  db.exec(SKEMA);
  seedTestData(db);
  db.close();
  return path;
}

export default { createDb, createDbFile, qListProduk, qListMember, qListKategoriMember, qListUsers, SKEMA, PIN_BCRYPT };
