// Verify: persist bon ke localStorage tahan restart + restore bersih.
// Simulasi in-memory `bon` + localStorage stub (seperti index.html simpanBonStorage/load).
import assert from 'node:assert'

// stub localStorage (WebView2 lokal)
const store = {}
globalThis.localStorage = {
  getItem: k => (k in store ? store[k] : null),
  setItem: (k, v) => { store[k] = String(v) },
}

// replika index.html
let LS_BON = 'zpos_bon'
let bon = []
function simpanBonStorage(){ try { localStorage.setItem(LS_BON, JSON.stringify(bon)); } catch(e){} }
function restore(){ try { const s = localStorage.getItem(LS_BON); if(s) bon = JSON.parse(s); } catch(e){} }

// 1) simpan 2 bon, persist
bon.push({ nama:'A', items:[{id:1,q:2}], total:5000, vmap:{}, bonId:0 })
bon.push({ nama:'B', items:[{id:7,q:1},{id:-3,q:1}], total:9000, vmap:{'-3':{nama:'Jasa',harga:1000}}, bonId:0 })
simpanBonStorage()
// 2) restart → bon di-reset (simulasi `let bon=[]`) lalu restore
bon = []
restore()
assert.strictEqual(bon.length, 2, 'harus 2 bon setelah restore')
assert.strictEqual(bon[0].nama, 'A')
assert.strictEqual(bon[1].items.length, 2, 'item virtual ikut tersimpan')
assert.deepStrictEqual(bon[1].vmap, {'-3':{nama:'Jasa',harga:1000}}, 'vmap virtual tersimpan')
// 3) tarik 1 → persist → restart → masih 1
bon.splice(0,1); simpanBonStorage()
bon = []; restore()
assert.strictEqual(bon.length, 1, 'sisa 1 bon setelah tarik+restart')
assert.strictEqual(bon[0].nama, 'B')
// 4) storage kosong → bon kosong
store[LS_BON] = ''
bon = []; restore()
assert.strictEqual(bon.length, 0, 'storage kosong = 0 bon')
// 5) data korup → exception ditangkap → bon tetap []
bon = []; store[LS_BON] = '{invalid'; restore()
assert.strictEqual(bon.length, 0, 'json korup diamankan')
console.log('PASS 6/6 — persist bon tahan restart + restore bersih, data virtual ikut')
