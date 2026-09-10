// Self-check: Lapis 1 (editPending+retry) & Lapis 2 (merge server-menang).
// Jalankan: node bon_sync_check.mjs  — TIRU logika index.html (bukan import;
// index.html = file HTML, helper tak dapat di-import). Tujuan: buktikan
// keputusan merge/pending benar utk skenario divergensi nyata.
// ponytail: mirror manual; kalau logika index.html berubah, sinkronkan 2 blok.

let fails = 0
const ck = (c, m) => { console.log((c ? 'ok   - ' : 'FAIL - ') + m); if (!c) fails++ }

// ---- TIRU mergeBonSync (Lapis 2) ----
function merge(bon, list, tarikBon) {
  const lokalOpts = {}
  for (const b of bon) if (b.bonId > 0) lokalOpts[b.bonId] = b
  let tambah = 0, timpa = 0
  for (const b of list) {
    const id = Number(b.id); if (!id) continue
    const hrs = (b.harga && typeof b.harga === 'object') ? b.harga : {}
    const items = Object.entries(b.produk || {})
      .map(([pid, q]) => ({ id: Number(pid), q: Number(q), h: (hrs[pid] != null ? Number(hrs[pid]) : undefined) }))
      .filter(it => it.id > 0 && it.q > 0)
    if (!items.length) continue
    const sesiServer = Array.isArray(b.sesi) && b.sesi.length
      ? b.sesi.map(s => ({ t: s.t, items: Object.entries(s.p || {}).map(([pid, q]) => ({ id: Number(pid), q: Number(q) })).filter(it => it.id > 0 && it.q > 0) })).filter(g => g.items.length)
      : null
    const lokal = lokalOpts[id]
    if (lokal) {
      if (lokal.lunasPending || lokal.hapusPending || lokal.editPending || lokal === tarikBon) continue
      lokal.nama = (b.nama || '').trim(); lokal.items = items; lokal.total = Number(b.total) || 0
      if (sesiServer) lokal.sess = sesiServer
      timpa++; continue
    }
    bon.push({ nama: (b.nama || '').trim(), items, total: Number(b.total) || 0, bonId: id, sess: sesiServer || undefined })
    tambah++
  }
  return { tambah, timpa }
}

// ---- TIRU retryEditBon (Lapis 1) ----
// kirim = async (b) => throw bila jaringan putus; sukses → set bonId bila create.
function retryEdit(bon, tarikBon, kirim) {
  const hasil = []
  for (const b of [...bon]) {
    if (!b.editPending || b.lunasPending || b.hapusPending || b === tarikBon) continue
    const webProduk = {}
    ;(b.items || []).forEach(it => { if (it.id > 0) webProduk[it.id] = (webProduk[it.id] || 0) + it.q })
    if (!Object.keys(webProduk).length) continue
    try {
      const id = kirim(b)             // throw → offline
      if (b.bonId > 0) { /* edit_bon */ } else { b.bonId = id }
      b.editPending = false; hasil.push('ok')
    } catch { hasil.push('gagal') }
  }
  return hasil
}

// ===== SKENARIO =====

// S1: bon kasir diedit GAGAL (offline) → editPending. Web masih versi lama.
{
  const bon = [{ bonId: 51, nama: 'PAK IDOI', items: [{ id: 7, q: 10, h: 2000 }], total: 20000, editPending: true }]
  const list = [{ id: 51, nama: 'PAK IDOI', produk: { 7: 2 }, total: 4000, harga: { 7: 2000 } }]
  const r = merge(bon, list, null)
  ck(r.timpa === 0, 'S1: bon editPending TIDAK ditimpa server (edit lokal dijaga)')
  ck(bon[0].items[0].q === 10, 'S1: isi lokal tetap 10 (tak balik ke 2)')
}

// S2: retry edit sukses → editPending hilang, isi terkirim.
{
  const bon = [{ bonId: 51, editPending: true, items: [{ id: 7, q: 10, h: 2000 }], total: 20000 }]
  const r = retryEdit(bon, null, () => 51)
  ck(r[0] === 'ok' && bon[0].editPending === false, 'S2: retry sukses → editPending clear')
}

// S3: retry edit GAGAL lagi → tetap pending (tak hilang).
{
  const bon = [{ bonId: 51, editPending: true, items: [{ id: 7, q: 10, h: 2000 }], total: 20000 }]
  const r = retryEdit(bon, null, () => { throw new Error('offline') })
  ck(r[0] === 'gagal' && bon[0].editPending === true, 'S3: retry gagal → tetap pending')
}

// S4: bon IDLE (tak pending) → ditimpa server (web diedit device lain).
{
  const bon = [{ bonId: 53, nama: 'MAMA AKMAL', items: [{ id: 9, q: 1, h: 5000 }], total: 5000 }]
  const list = [{ id: 53, nama: 'MAMA AKMAL', produk: { 9: 3 }, total: 15000, harga: { 9: 5000 } }]
  const r = merge(bon, list, null)
  ck(r.timpa === 1 && bon[0].items[0].q === 3, 'S4: bon IDLE ditimpa → qty ikut server (3)')
  ck(bon[0].total === 15000, 'S4: total ikut server')
}

// S5: bon sedang DITARIK → tak ditimpa.
{
  const b = { bonId: 54, items: [{ id: 9, q: 5, h: 5000 }], total: 25000 }
  const bon = [b]
  const r = merge(bon, [{ id: 54, produk: { 9: 1 }, total: 5000, harga: { 9: 5000 } }], b)
  ck(r.timpa === 0 && b.items[0].q === 5, 'S5: _tarikBon tak ditimpa (keranjang kasir aman)')
}

// S6: bon BARU dari server → ditambah.
{
  const bon = []
  const r = merge(bon, [{ id: 65, nama: 'TEST1', produk: { 7: 5 }, total: 10000, harga: { 7: 2000 } }], null)
  ck(r.tambah === 1 && bon[0].bonId === 65 && bon[0].items[0].h === 2000, 'S6: bon baru + harga terkunci masuk')
}

// S7: harga terkunci server MENANG atas apa pun di jalur timpa.
{
  const bon = [{ bonId: 51, items: [{ id: 7, q: 10, h: 9999 }], total: 0 }]
  merge(bon, [{ id: 51, produk: { 7: 10 }, total: 20000, harga: { 7: 2000 } }], null)
  ck(bon[0].items[0].h === 2000, 'S7: harga terkunci server (2000) menang')
}

// S8: server tak kirim harga (bon lama) → h undefined, bukan angka palsu.
{
  const bon = [{ bonId: 60, items: [{ id: 7, q: 10, h: 2000 }], total: 20000 }]
  merge(bon, [{ id: 60, produk: { 7: 10 }, total: 20000 }], null)
  ck(bon[0].items[0].h === undefined, 'S8: tanpa harga server → h undefined (fallback katalog kasir)')
}

console.log('\n' + (fails ? `${fails} FAILED` : 'ALL PASS'))
process.exit(fails ? 1 : 0)
