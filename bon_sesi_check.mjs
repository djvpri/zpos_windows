// Self-check harga PER-SESI bon gantung (kasir ↔ server).
// Skenario nyata bon #103 tenant Demo: Ayam Geprek #175 digantung 09-09 saat
// katalog 21.000 (grup 1), lalu ditambah 10-09 saat katalog sudah 15.000 (grup 4).
// Harapan: grup 1 tetap 21.000, grup 4 = 15.000, total = Σ per-sesi.
// Jalankan: node bon_sesi_check.mjs
import assert from 'node:assert/strict'

// --- tiruan util server (lib/bon-sesi.ts) ---
const parseHargaMap = (v) => {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return undefined
  const out = {}
  for (const [k, x] of Object.entries(v)) { const n = Number(x); if (Number.isFinite(n)) out[k] = n }
  return Object.keys(out).length ? out : undefined
}
const normalSesi = (raw) => {
  if (!Array.isArray(raw)) return []
  const out = []
  for (const g of raw) {
    if (!g || typeof g !== 'object') continue
    const t = typeof g.t === 'string' ? g.t : null
    const pRaw = g.p
    if (!t || !pRaw || typeof pRaw !== 'object' || Array.isArray(pRaw)) continue
    const p = {}
    for (const [id, q] of Object.entries(pRaw)) {
      const n = Number(id), qq = Number(q)
      if (Number.isInteger(n) && n > 0 && Number.isInteger(qq) && qq > 0) p[String(n)] = qq
    }
    if (!Object.keys(p).length) continue
    const s = { t, p }
    const h = parseHargaMap(g.h)
    if (h) { const hh = {}; for (const [id, v] of Object.entries(h)) if (p[id] != null) hh[id] = Math.round(v); if (Object.keys(hh).length) s.h = hh }
    out.push(s)
  }
  return out
}
const deltaPositif = (sebelum, kini) => {
  const out = {}
  for (const [id, q] of Object.entries(kini)) { const d = q - (sebelum ? sebelum[id] || 0 : 0); if (d > 0) out[id] = d }
  return out
}

// --- harga efektif per produk, prioritas sesi.h → hargaKunci → katalog (tiruan nota) ---
const hargaEfektif = (daftarSesi, hargaKunci, katalog, pid) => {
  let q = 0, sum = 0
  for (let i = 0; i < daftarSesi.length; i++) {
    const s = daftarSesi[i]
    const prev = i === 0 ? null : daftarSesi[i - 1].p
    const d = i === 0 ? Number(s.p[String(pid)] || 0) : Number(deltaPositif(prev ?? {}, s.p)[String(pid)] || 0)
    if (d <= 0) continue
    const hs = s.h ? Number(s.h[String(pid)]) : NaN
    const h = Number.isFinite(hs) ? hs : (hargaKunci[String(pid)] != null ? Number(hargaKunci[String(pid)]) : (katalog[pid] ?? 0))
    q += d; sum += h * d
  }
  if (q > 0) return sum / q
  const k = hargaKunci[String(pid)]
  return (k != null && Number.isFinite(Number(k))) ? Number(k) : (katalog[pid] ?? 0)
}

// --- kasir: rebuildSess (tiruan 1:1 src/index.html) — grup baru pakai harga
// katalog SAAT ITU (`hNow2`), grup lama pertahankan `it.h` ---
const rebuildSess = (oldSess, kart, katalogNow) => {
  const need = {}
  for (const c of kart) need[c.id] = (need[c.id] || 0) + (c.q || 0)
  const hNow = {}
  for (const c of kart) if (hNow[c.id] == null && katalogNow[c.id] != null) hNow[c.id] = katalogNow[c.id]
  const out = []
  for (const g of (oldSess || [])) {
    const its = []
    for (const it of (g.items || [])) {
      const rem = need[it.id]; if (!rem) continue
      const take = Math.min(it.q, rem)
      const h = (it.h != null) ? it.h : hNow[it.id]
      its.push(h != null ? { id: it.id, q: take, h } : { id: it.id, q: take })
      need[it.id] = rem - take
    }
    if (its.length) out.push({ ts: (g.ts != null) ? g.ts : Date.now(), items: its })
  }
  const left = Object.entries(need).filter(([, q]) => q > 0)
  if (left.length) out.push({ ts: Date.now(), items: left.map(([id, q]) => { const h = hNow[Number(id)]; return h != null ? { id: Number(id), q, h } : { id: Number(id), q } }) })
  return out
}

let n = 0
const ok = (name, fn) => { fn(); n++; console.log(`ok ${n} - ${name}`) }

const KATALOG_SEKARANG = { 175: 15000, 184: 65000 }
const HARGA_KUNCI_BON = { 175: 21000, 184: 65000 }

// 1) grup lama (09-09, katalog 21.000) → sesi.h = 21.000
ok('grup 1 kunci 21.000 (harga saat dibuat)', () => {
  const sess = [{ ts: 1, items: [{ id: 175, q: 1, h: 21000 }, { id: 184, q: 1, h: 65000 }] }]
  assert.equal(sess[0].items[0].h, 21000)
  const s = normalSesi([{ t: new Date(1).toISOString(), p: { 175: 1, 184: 1 }, h: { 175: 21000, 184: 65000 } }])
  assert.equal(s[0].h['175'], 21000)
})

// 2) tambahan 10-09 (katalog sudah 15.000) → grup baru h = 15.000 (BUKAN kunci 21.000)
ok('grup 4 tambahan pakai harga katalog saat itu (15.000)', () => {
  const oldSess = [{ ts: 1, items: [{ id: 175, q: 1, h: 21000 }] }]
  const kart = [{ id: 175, q: 4 }]                       // total jadi 4 (1 lama + 3 baru)
  const sess = rebuildSess(oldSess, kart, KATALOG_SEKARANG)
  assert.equal(sess.length, 2, 'harus 2 grup')
  assert.equal(sess[0].items[0].h, 21000, 'grup 1 tetap 21.000')
  assert.equal(sess[1].items[0].q, 3, 'grup baru qty 3')
  assert.equal(sess[1].items[0].h, 15000, 'grup 4 = katalog saat itu 15.000')
})

// 3) endpoint nota: grup pakai sesi.h masing-masing
ok('nota grup: 1→21.000, 4→15.000', () => {
  const daftarSesi = normalSesi([
    { t: '2026-09-09T07:41:00.000Z', p: { 175: 1 }, h: { 175: 21000 } },
    { t: '2026-09-10T08:55:00.000Z', p: { 175: 4 }, h: { 175: 15000 } },
  ])
  const g1 = daftarSesi.map((s, i) => ({ sesiNo: i + 1, m: i === 0 ? s.p : deltaPositif(daftarSesi[i - 1].p, s.p), s }))
  const h = (g, pid) => { const hs = g.s.h ? Number(g.s.h[String(pid)]) : NaN; return Number.isFinite(hs) ? hs : HARGA_KUNCI_BON[pid] }
  assert.equal(h(g1[0], 175), 21000, 'grup 1 = 21.000')
  assert.equal(h(g1[1], 175), 15000, 'grup 4 = 15.000')
  // grup 4 delta qty = 3 (4 total - 1 lama)
  assert.equal(g1[1].m['175'], 3)
})

// 4) items flat = rata-rata tertimbang (1×21.000 + 3×15.000)/4 = 16.500
ok('items flat = rata-rata tertimbang 16.500', () => {
  const daftarSesi = normalSesi([
    { t: '2026-09-09T07:41:00.000Z', p: { 175: 1 }, h: { 175: 21000 } },
    { t: '2026-09-10T08:55:00.000Z', p: { 175: 4 }, h: { 175: 15000 } },
  ])
  assert.equal(hargaEfektif(daftarSesi, HARGA_KUNCI_BON, KATALOG_SEKARANG, 175), 16500)
})

// 5) total bon = Σ per-sesi (bukan katalog × qty)
ok('total = Σ per-sesi = 1×21000 + 3×15000 = 66.000', () => {
  const daftarSesi = normalSesi([
    { t: 'x', p: { 175: 1 }, h: { 175: 21000 } },
    { t: 'y', p: { 175: 4 }, h: { 175: 15000 } },
  ])
  let total = 0
  for (let i = 0; i < daftarSesi.length; i++) {
    const m = i === 0 ? daftarSesi[i].p : deltaPositif(daftarSesi[i - 1].p, daftarSesi[i].p)
    for (const [pid, q] of Object.entries(m)) total += Number(daftarSesi[i].h[pid]) * q
  }
  assert.equal(total, 66000)
})

// 6) bon lama (sesi tanpa h) → fallback harga_json global (perilaku lama tak rusak)
ok('bon lama tanpa sesi.h → fallback harga_json', () => {
  const daftarSesi = normalSesi([{ t: 'x', p: { 175: 1 } }])   // tak ada h
  assert.equal(hargaEfektif(daftarSesi, HARGA_KUNCI_BON, KATALOG_SEKARANG, 175), 21000)
})

// 7) bon lama tanpa h & tanpa harga_json → fallback katalog
ok('bon lama tanpa h & harga_json → katalog', () => {
  const daftarSesi = normalSesi([{ t: 'x', p: { 175: 1 } }])
  assert.equal(hargaEfektif(daftarSesi, {}, KATALOG_SEKARANG, 175), 15000)
})

// 8) normalSesi buang h utk produk yg tak ada di sesi itu (anti-drift)
ok('normalSesi selaraskan h dgn p', () => {
  const s = normalSesi([{ t: 'x', p: { 175: 1 }, h: { 175: 21000, 999: 5 } }])
  assert.deepEqual(Object.keys(s[0].h), ['175'])
})

// 9) normalSesi tolak id/qty non-positif & bentuk rusak
ok('normalSesi sanitasi input', () => {
  assert.deepEqual(normalSesi([{ t: 'x', p: { 0: 1, '-2': 3, 175: 0, 184: 2 } }])[0].p, { 184: 2 })
  assert.deepEqual(normalSesi('bukan array'), [])
  assert.deepEqual(normalSesi([{ p: { 175: 1 } }]), [])   // tanpa t
})

console.log(`\n${n}/9 PASS`)
