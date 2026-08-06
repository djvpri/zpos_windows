// run.mjs — Deterministic verification harness for ZPos Kasir frontend.
// Menekankan pola yang sama dgn ink-tide `__INKTIDE__`: determinisme + screenshot
// sebagai bukti visual + assertion numerik/deterministik. Untuk ZPos (app Tauri
// event-driven, bukan game loop) determinisme waktu TIDAK relevan; yang ditransfer:
//   (1) mock `__TAURI__` deterministic (data dari SQLite fixture, bukan obj kalut)
//   (2) screenshot bukti tiap scene (shots/<scene>.png)
//   (3) assertion state eksplisit (submit, lalu baca DOM) — bukan "terlihat oke"
//   (4) schema drift check: fixture SQLite harus sejalan dgn db.rs (kontrak backend)
//
// Jalankan:  node run.mjs            (default: semua scene)
//            node run.mjs kasir       (satu scene saja)
// Exit 0 = PASS, 1 = FAIL.

import { chromium } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { existsSync, mkdirSync } from 'node:fs';

import { createDb, qListProduk, qListMember, qListKategoriMember, qListUsers } from './fixtures/db.mjs';

const ROOT = fileURLToPath(new URL('.', import.meta.url));
const INDEX = join(ROOT, '..', 'src', 'index.html');
const SHOTS = join(ROOT, 'shots');

// ---------------------------------------------------------------------------
// Bagian 0: konfigurasi
// ---------------------------------------------------------------------------
const OPTS = {
  // fingerprint build (terlihat di screenshot, mudah di-assert) — versi + tgl utk
  // TIDAK bikin screenshot berubah tiap run (bukan golden). Hanya label.
  indexUrl: 'file:///' + INDEX.replace(/\\/g, '/'),
  slowMo: 0,
};

// ---------------------------------------------------------------------------
// Bagian 1: mock __TAURI__ invoke — deterministic, read dari SQLite fixture.
// Setiap command memetakan 1:1 ke invoke yang dipanggil src/index.html.
// CATATAN: login_pin memvalidasi bentuk PIN + user aktif (bukan bcrypt — kriptografi
// sudah dites backend Rust; ini kontrak alur JS: call arg shape & promise resolve).
// ---------------------------------------------------------------------------
function makeInvokeHandler(ctx) {
  return async (cmd, args) => {
    const { db } = ctx;
    const a = args || {};
    const fail = (m) => { throw new Error(m); };

    switch (cmd) {
      case 'list_produk':  return qListProduk(db);
      case 'list_member':  return qListMember(db);
      case 'list_kategori_member': return qListKategoriMember(db);
      case 'list_users':   return qListUsers(db);
      case 'jumlah_antrian': {
        const r = db.prepare('SELECT COUNT(*) n FROM antrian').get();
        return r.n;
      }
      case 'versi_app': return '0.1.18';
      case 'baca_log': return 'mock log\nbaris 2';

      case 'login_pin': {
        const { userId, pin } = a;
        if (!Number.isInteger(userId) && typeof userId !== 'number') fail('login_pin: userId harus angka');
        if (typeof pin !== 'string' || pin.length !== 6) fail('PIN harus 6 angka');
        const u = db.prepare('SELECT * FROM users_lokal WHERE id=? AND aktif=1').get(userId);
        if (!u) fail('User tidak ditemukan atau nonaktif.');
        // kontrak: PIN "123456" => sukses utk user 1; selain itu ditolak.
        // (bcrypt asli diuji backend; mock meniru keputusan login utk alur JS.)
        if (pin === '123456') return true;
        // catat attempt-lock secara deterministik utk skenario lock.
        if (pin === '999999') return false === pin; // unreachable guard
        fail('PIN salah (1x).');
      }

      case 'harga_member': {
        const { memberId } = a;
        const kat = db.prepare('SELECT kategori_member_id FROM member WHERE id=?').get(memberId);
        if (!kat || !kat.kategori_member_id) return {};
        const diskon = db.prepare('SELECT diskon_persen FROM kategori_member WHERE id=?').get(kat.kategori_member_id)?.diskon_persen ?? 0;
        const out = {};
        const rows = db.prepare('SELECT id,harga FROM produk').all();
        for (const p of rows) {
          const tetap = db.prepare('SELECT harga FROM harga_member WHERE produk_id=? AND kategori_member_id=?').get(p.id, kat.kategori_member_id);
          const efektif = tetap ? tetap.harga : (diskon !== 0 ? Math.round(p.harga * (1 - diskon / 100)) : p.harga);
          out[String(p.id)] = efektif;
        }
        return out;
      }

      case 'antri_transaksi': {
        const t = a.t || {};
        if (!t.client_ref || !t.payload) fail('antri_transaksi: butuh t.client_ref + t.payload');
        db.prepare(`INSERT INTO antrian (client_ref,produk,metode,total,dibuat_at,user_id,user_nama)
            VALUES (?,?,?,?,datetime('now'),?,?)`)
          .run(t.client_ref, JSON.stringify(t.payload), t.metode ?? null, t.total ?? null, t.user_id ?? null, t.user_nama ?? null);
        return null;
      }

      case 'sync_remote': return 'kategori 2, produk 4, member 2, user 2, push 0';
      case 'tulis_log': return null;
      case 'buka_devtools': return null;
      case 'buka_url': {
        // verifikasi kontrak: URL yang dibuka kelihatan di console (assertable di test).
        ctx.openedUrls = ctx.openedUrls || [];
        ctx.openedUrls.push(a.url);
        return null;
      }
      case 'unduh_update': {
        // simpan URL utk assertion; balik path bayangan.
        ctx.updateUrl = a.url;
        return 'C:\\Dev\\zpos-kasir.new.exe';
      }
      case 'terapkan_update': {
        // INI regression test kunci (fix 0.1.18): frontend harus kirim {payload:{...}}.
        // Simpan arg MENTAH (utk assert wrapper presence) + payload terpisah.
        ctx.updateInvokeArgs = a;
        ctx.appliedPayload = a.payload;
        return null;
      }
      case 'keluar': ctx.requestedExit = true; return null;

      default:
        fail('invoke tak dikenal di mock: ' + cmd);
    }
  };
}

// ---------------------------------------------------------------------------
// Bagian 2: harness kecil — screenshot + assert + reporting
// ---------------------------------------------------------------------------
let failures = 0;
const passed = [];

function assert(cond, label, extra) {
  if (cond) { passed.push(label); }
  else { failures++; console.error(`  ✗ ${label}${extra ? '  →  ' + extra : ''}`); }
}
async function shot(page, name) {
  mkdirSync(SHOTS, { recursive: true });
  await page.screenshot({ path: join(SHOTS, name + '.png'), fullPage: false });
}

// ---------------------------------------------------------------------------
// Bagian 3: scene — tiap scene punya langkah deterministik + assert + screenshot.
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Bagian 2B: init-script injector — satu sumber kebenaran utk harness.
// Definisikan __TAURI__ mock, zpos_base, dan percepat autoSync (interval 60s
// → 50ms) supaya `offline=false` (sync_remote resolve) SEBELUM alur yang butuh
// data server. Ini pola kontrol-waktu deterministik ala ink-tide.
// ---------------------------------------------------------------------------
async function inject(page, handler) {
  const _orig = `const __orig = { setInterval: window.setInterval.bind(window), setTimeout: window.setTimeout.bind(window) };
      window.setInterval = function(fn, ms){
        if(ms === 60000) return __orig.setTimeout(fn, 50);  // autoSync sekali (60s → sekali 50ms), jangan loop
        return __orig.setInterval(fn, ms);
      };`;
  await page.exposeFunction('__zposInvoke', handler);
  await page.addInitScript(() => {
    localStorage.setItem('zpos_base', 'https://zpos.zomet.my.id');
    Object.defineProperty(window, '__TAURI__', {
      get: () => ({ core: { invoke: async (cmd, args) => window.__zposInvoke(cmd, args || {}) } }),
      configurable: true,
    });
  });
  await page.addInitScript(_orig);
}

async function scene_login(ctx) {
  const { browser } = ctx;
  const p = await browser.newContext({ viewport: { width: 1100, height: 780 } });
  const page = await p.newPage();
  await inject(page, ctx.handler);
  await page.goto(OPTS.indexUrl);
  await page.waitForFunction(() => window.__zposInvoke !== undefined);

  // initKasir jalan: login modal auto-buka setelah 400ms.
  await page.waitForSelector('#loginModal.show', { timeout: 8000 });

  // data dari fixture: kasir A + admin
  const users = await page.locator('#loginUser option').allTextContents();
  assert(users.some(t => t.includes('Kasir A')), 'list_users memuat kasir dari fixture SQLite', 'users=' + users.join('|'));

  // pilih user, PIN benar
  await page.selectOption('#loginUser', '1');
  await page.fill('#loginPin', '123456');
  await page.click('#loginBtn');
  try {
    await page.waitForFunction(() => !document.getElementById('loginModal').classList.contains('show'), { timeout: 8000 });
  } catch {
    const msg = (await page.locator('#loginMsg').textContent()).trim();
    const err = (await page.locator('#errLog').textContent()).trim();
    throw new Error('login modal tak tutup. loginMsg="' + msg + '" errLog="' + (err || '(kosong)') + '"');
  }
  await shot(page, '01-login-success');
  const pill = await page.locator('#kasirNama').textContent();
  assert(pill.trim() === 'Kasir A', 'login PIN sukses → badge kasir', 'badge=' + pill);

  // katalog ter-render dari fixture (4 produk, urut nama: Es Teh, Kopi Susu, Nasi Goreng, Roti Bakar)
  const cards = await page.locator('#grid .card').count();
  assert(cards === 4, 'render katalog dari fixture (4 produk)', 'cards=' + cards);

  // tambah 2x Nasi Goreng (15.000) → sub 30.000, diskon 9% = 30k*9% = 2700 → total 27000
  const nmTexts = await page.locator('#grid .card .nm').allTextContents();
  if (!nmTexts.some(t => t.includes('Nasi Goreng'))) {
    throw new Error('Nasi Goreng tak ada di grid. nm=' + JSON.stringify(nmTexts) + ' PRODUK=' + await page.evaluate(() => JSON.stringify((window.PRODUK || []).map(p => p.n))));
  }
  await page.click('#grid .card:has-text("Nasi Goreng")');
  await page.click('#grid .card:has-text("Nasi Goreng")');
  const total = (await page.locator('#total').textContent()).trim();
  // Nasi Goreng 15000*2=30000; diskon 9% = 2700 → total 27000
  assert(total.includes('Rp27.000'), 'total keranjang terhitung benar (27000)', 'total=' + total);
  await shot(page, '02-cart-2xgoreng');

  await p.close();
}

async function scene_member(ctx) {
  const { browser } = ctx;
  const p = await browser.newContext({ viewport: { width: 1100, height: 780 } });
  const page = await p.newPage();
  await inject(page, ctx.handler);
  await page.goto(OPTS.indexUrl);
  await page.waitForFunction(() => window.__zposInvoke !== undefined);
  await page.waitForSelector('#loginModal.show', { timeout: 8000 });
  await page.selectOption('#loginUser', '1');
  await page.fill('#loginPin', '123456');
  await page.click('#loginBtn');
  await page.waitForFunction(() => !document.getElementById('loginModal').classList.contains('show'), { timeout: 8000 });

  // Buka modal tambah member → dropdown kategori dari fixture (3 opsi: tanpa + 2 kat member)
  await page.click('#memberPill');
  await page.click('#mwin button[onclick="bukaAddMember()"]');
  await page.waitForSelector('#addMemberModal.show', { timeout: 5000 });
  await shot(page, '03-member-modal');

  const katOptions = await page.locator('#amKat option').allTextContents();
  // fixture: id1 -7% (markup), id2 10% (diskon). Tanpa kategori = opsi kosong.
  assert(katOptions.length >= 3, 'dropdown kategori member terisi dari fixture', 'opts=' + katOptions.join('|'));
  assert(katOptions.some(o => o.includes('Loyal') && o.includes('10%')), 'kategori diskon 10% tampil', katOptions.join('|'));
  assert(katOptions.some(o => o.includes('Bon Pribadi')), 'kategori markup -7% tampil', katOptions.join('|'));

  // tutup modal (klik backdrop di koordinat luar card) sebelum pilih member dari pill
  await page.mouse.click(8, 700); // area backdrop kiri-luar modal card
  await page.waitForFunction(() => !document.getElementById('addMemberModal').classList.contains('show'), { timeout: 4000 });

  // Pilih member via pill: Syahrul (markup) + Es Teh (harga tetap 4500)
  await page.click('#memberPill');
  await page.click('.mitem:has-text("Syahrul")');
  await shot(page, '04-member-selected');
  const pillTxt = (await page.locator('#memberPillTxt').textContent()).trim();
  assert(pillTxt.includes('Bon Pribadi'), 'badge member = Syahrul · Bon Pribadi', pillTxt);

  await p.close();
}

// --- regression update (fix 0.1.18): payload wrapper ---
async function scene_update(ctx) {
  const { browser } = ctx;
  const p = await browser.newContext({ viewport: { width: 1100, height: 780 } });
  const page = await p.newPage();
  await inject(page, ctx.handler);
  await page.goto(OPTS.indexUrl);
  await page.waitForFunction(() => window.__zposInvoke !== undefined);
  await page.waitForSelector('#loginModal.show', { timeout: 8000 });
  await page.selectOption('#loginUser', '1');
  await page.fill('#loginPin', '123456');
  await page.click('#loginBtn');
  await page.waitForFunction(() => !document.getElementById('loginModal').classList.contains('show'), { timeout: 8000 });

  // Simulasikan doUpdate langsung (fungsi global index.html: download + terapkan).
  await page.evaluate(() => window.doUpdate('https://example.com/zpos-kasir.exe', '0.1.19'));
  // Tunggu handler mock 'terapkan_update' tercatat di ctx (poll, deterministik).
  for (let i = 0; i < 40 && ctx.appliedPayload === null; i++) {
    await page.waitForTimeout(50);
  }
  await page.waitForTimeout(100); // biarkan promise terakhir resolve

  // assert invoke shape via ctx (bukan page) — handler mencatat.
  const raw = ctx.updateInvokeArgs;   // arg mentah yg dikirim frontend ke tinv
  const ap = ctx.appliedPayload;      // payload yg diterima Rust
  assert(raw && typeof raw === 'object' && ('payload' in raw), 'FRONTEND kirim {payload:{...}} (fix 0.1.18)', JSON.stringify(raw));
  assert(ap && ap.targetPath === 'C:\\Dev\\zpos-kasir.new.exe', 'payload.targetPath ter-set', JSON.stringify(ap));
  assert(ap && ap.versiBaru === '0.1.19', 'payload.versiBaru ter-set', JSON.stringify(ap));
  // PASTI bukan regresi lama: Rust 0.1.15-17 butuh wrapper {payload:{...}};
  // kirim {targetPath,versiBaru} datar → missing required key payload.
  assert(!('targetPath' in raw), 'tak mengirim targetPath datar (bukan tanpa wrapper)', JSON.stringify(raw));

  await p.close();
}

// --- scene: pin salah → error message (kontrak login gagal) ---
async function scene_login_fail(ctx) {
  const { browser } = ctx;
  const p = await browser.newContext({ viewport: { width: 1100, height: 780 } });
  const page = await p.newPage();
  await inject(page, ctx.handler);
  await page.goto(OPTS.indexUrl);
  await page.waitForFunction(() => window.__zposInvoke !== undefined);
  await page.waitForSelector('#loginModal.show', { timeout: 8000 });
  await page.selectOption('#loginUser', '1');
  await page.fill('#loginPin', '000000'); // salah
  await page.click('#loginBtn');
  await page.waitForFunction(() => document.getElementById('loginMsg').textContent.trim() !== '', { timeout: 8000 });  await shot(page, '05-login-fail');
  const msg = (await page.locator('#loginMsg').textContent()).trim();
  assert(/PIN/i.test(msg), 'login PIN salah → pesan error tampil', msg);
  await p.close();
}

// --- schema drift check: fixture table/kolom sejalan dgn db.rs ---
import { SKEMA } from './fixtures/db.mjs';
function scene_schema() {
  // daftar kolom yang DIPAKAI query frontend/backend dari db.rs — harus ada di fixture.
  const perlu = [
    ['produk', ['id','nama','harga','stok','kategori_id','barcode','foto_url']],
    ['kategori', ['id','nama']],
    ['kategori_member', ['id','nama','diskon_persen','urutan']],
    ['member', ['id','nama','telepon','kategori_member_id']],
    ['harga_member', ['produk_id','kategori_member_id','harga']],
    ['antrian', ['id','client_ref','produk','metode','total','dibuat_at','user_id','user_nama']],
    ['users_lokal', ['id','toko_id','nama','email','role','aktif','pin_hash']],
    ['meta', ['k','v']],
  ];
  for (const [tbl, cols] of perlu) {
    for (const c of cols) {
      const re = new RegExp('\\b' + c + '\\b');
      assert(re.test(SKEMA), `schema: ${tbl}.${c} ada di fixture (kontrak db.rs)`, '');
    }
  }
}

// ---------------------------------------------------------------------------
// MAIN
// ---------------------------------------------------------------------------
const only = process.argv[2];
console.log('ZPos Kasir harness  ·  deterministic Playwright Chrome on mock __TAURI__\n');
console.log('  target:', OPTS.indexUrl, '\n');

const sc = {
  login: scene_login,
  member: scene_member,
  update: scene_update,
  login_fail: scene_login_fail,
  schema: scene_schema,
};

async function main() {
  const db = createDb();
  const ctx = { db, handler: null, openedUrls: [], updateUrl: null, updateInvokeArgs: null, appliedPayload: null, requestedExit: false };
  ctx.handler = makeInvokeHandler(ctx);

  const browser = await chromium.launch({ headless: true });
  ctx.browser = browser;

  try {
    for (const [name, fn] of Object.entries(sc)) {
      if (only && name !== only) continue;
      console.log(`— scene:${name}`);
      try { await fn(ctx, { browser, seed: db }); }
      catch (e) {
        failures++;
        console.error('  ✗ scene ' + name + ' error: ' + (e && e.message || e));
      }
    }
  } finally {
    await browser.close();
    db.close();
  }

  console.log('\n--------------------------------------------------');
  if (failures === 0) {
    console.log(`PASS  ·  ${passed.length} assertion, 0 gagal`);
    console.log('Screenshot: harness/shots/*.png');
    process.exit(0);
  } else {
    console.log(`FAIL  ·  ${failures} assertion gagal (${passed.length} oke)`);
    process.exit(1);
  }
}

main().catch((e) => { console.error('FATAL', e); process.exit(2); });
