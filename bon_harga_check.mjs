// Self-check: perilaku harga terkunci bon (Opsi A). Jalankan: node bon_harga_check.mjs
// TIRU helper frontend (hargaIt) + alur tarik bon vs harga katalog berubah.
// ponytail: harness terpisah dari index.html; kalau helper berubah, sinkronkan manual.

const PRODUK = [{ id: 7, n: 'Telur', p: 2000 }];        // katalog saat bon DIGANTUNG
const virtualProduk = {};
const pCart = (id) => PRODUK.find((p) => p.id === id) || null;
const hargaMap = {};               // harga member (kosong = tanpa member)
const member = null;
function hargaMember(id) {
  if (!member) return null;
  if (hargaMap[id] != null) return hargaMap[id];
  const p = PRODUK.find((p) => p.id === id);
  return p ? p.p : null;
}
function hargaIt(c) {
  if (c && c.h != null) return c.h;
  if (c && c.id < 0 && virtualProduk[c.id]) return virtualProduk[c.id].harga;
  const p = pCart(c.id);
  return hargaMember(c.id) != null ? hargaMember(c.id) : p ? p.p : 0;
}
const sub = (cart) => cart.reduce((s, c) => s + (pCart(c.id) ? hargaIt(c) * c.q : 0), 0);

let fail = 0;
const ok = (cond, msg) => { console.log((cond ? 'ok  ' : 'FAIL') + ' - ' + msg); if (!cond) fail++; };

// 1) Hari-1: gantung bon telur qty 3 @2000 → items simpan h=2000
const cartHari1 = [{ id: 7, q: 3 }];
const itemsBon = cartHari1.map((c) => ({ id: c.id, q: c.q, h: hargaIt(c) }));
ok(itemsBon[0].h === 2000, 'bon simpan harga terkunci 2000 saat digantung');
ok(sub(cartHari1) === 6000, 'total bon hari-1 = 6000');

// 2) Hari-2: katalog telur naik 2000 -> 2500
PRODUK[0].p = 2500;

// 3) Tarik bon → cart bawa h dari item bon
const cartTarik = itemsBon.map((it) => ({ id: it.id, q: it.q, h: it.h }));
ok(hargaIt(cartTarik[0]) === 2000, 'tarik bon pakai harga TERKUNCI 2000 (bukan 2500)');
ok(sub(cartTarik) === 6000, 'total saat tarik tetap 6000 (harga bon tak ikut naik)');

// 4) Bon lama tanpa h → fallback harga katalog terkini (backward-compat)
const cartLama = [{ id: 7, q: 2 }];
ok(hargaIt(cartLama[0]) === 2500, 'bon lama tanpa h → pakai harga katalog 2500 (kompat)');

// 5) Keranjang biasa (bukan dari bon) → harga katalog terkini
ok(hargaIt({ id: 7, q: 1 }) === 2500, 'keranjang baru pakai harga katalog terkini');

// 6) Merge dari server: item dapat h dari map harga server
const bServer = { harga: { 7: 2000 }, produk: { 7: 3 } };
const hrs = bServer.harga && typeof bServer.harga === 'object' ? bServer.harga : {};
const merged = Object.entries(bServer.produk).map(([pid, q]) => ({ id: Number(pid), q: Number(q), h: hrs[pid] != null ? Number(hrs[pid]) : undefined }));
ok(merged[0].h === 2000, 'merge bon dari server bawa harga terkunci 2000');

console.log(fail === 0 ? '\nALL PASS' : `\n${fail} FAILED`);
process.exit(fail === 0 ? 0 : 1);
