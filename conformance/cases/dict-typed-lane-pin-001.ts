// A `Record<string, number>` read followed by a write of the same key
// (`d[k] = (d[k] ?? 0) + x`) answers what two separate §10.1.8.1
// [[Get]] / §10.1.9.2 [[Set]] steps answer, even when the right-hand
// side changes the dictionary between them: deletes the key, grows the
// table, makes the entry read-only, or writes the same key through an
// `any` alias. Keys equal by content but built separately, keys that
// differ only in the UTF-16 encoding, and symbol entries share the
// table with them; the inherited names (`__proto__`, `toString`, a
// field added to Object.prototype) and an `any` string value read as
// the generic read does.
const ks: string[] = ["k1", "k2", "__proto__", "toString", "extra", "😀", "é"]
const d: Record<string, number> = {}
const e: Record<string, number> = {}
const alias: any = d
let grow = 0

function del(k: string): number {
  delete d[k]
  return 1
}
function fill(n: number): number {
  for (let i = 0; i < n; i = i + 1) d["f" + grow + "_" + i] = i
  grow = grow + 1
  return 2
}
function freezeKey(k: string): number {
  Object.defineProperty(d, k, { writable: false })
  return 3
}
function viaAlias(k: string): number {
  alias[k] = "s"
  return 4
}

d[ks[0]] = 1
d[ks[1]] = 2
e[ks[1]] = 20
e[ks[0]] = 10
d[ks[0]] = (d[ks[0]] ?? 0) + del(ks[0])
console.log(d[ks[0]], Object.keys(d).join(","))
d[ks[1]] = (d[ks[1]] ?? 0) + fill(40)
console.log(d[ks[1]], Object.keys(d).length)
e[ks[1]] = (d[ks[1]] ?? 0) + 1
e[ks[0]] = (e[ks[0]] ?? 0) + (d[ks[0]] ?? 0)
console.log(e[ks[0]], e[ks[1]], d[ks[0]], d[ks[1]])
d[ks[0]] = (d[ks[0]] ?? 0) + viaAlias(ks[0])
console.log(d[ks[0]], typeof d[ks[0]])
try {
  d[ks[1]] = (d[ks[1]] ?? 0) + freezeKey(ks[1])
  console.log("wrote")
} catch (err) {
  console.log(err instanceof TypeError, d[ks[1]])
}

const built = "k" + (1 + 1)
const other = ["k", "2"].join("")
console.log(d[built], d[other], built === ks[1])
d[ks[5]] = 5
d[ks[6]] = 6
const wide = String.fromCharCode(0xd83d, 0xde00)
const narrow = String.fromCharCode(0xe9)
d[wide] = (d[wide] ?? 0) + 1
d[narrow] = (d[narrow] ?? 0) + 1
console.log(d[ks[5]], d[ks[6]], d["é"], d["😀"])

const sym = Symbol("k1")
const ds: any = d
ds[sym] = 7
d[ks[0]] = 9
console.log(d[ks[0]], ds[sym], d[String(ks[0])])

Object.defineProperty(Object.prototype, "extra", { value: 11, writable: true, configurable: true })
console.log(d[ks[4]], (d[ks[4]] ?? 0) + 1)
d[ks[4]] = (d[ks[4]] ?? 0) + 1
console.log(d[ks[4]], Object.prototype.hasOwnProperty.call(d, ks[4]), ({} as any).extra)
delete (Object.prototype as any).extra
console.log(typeof d[ks[3]], d[ks[2]] === Object.prototype)
d[ks[3]] = (d[ks[3]] ?? 0) + 1
console.log(typeof d[ks[3]], Object.prototype.hasOwnProperty.call(d, ks[3]))
delete d[ks[3]]
console.log(typeof d[ks[3]], d[ks[3]] === Object.prototype.toString)

const counts: Record<string, number> = {}
const keys: string[] = []
for (let i = 0; i < 64; i = i + 1) keys.push("k" + i)
for (let i = 0; i < 1000; i = i + 1) {
  const k = keys[(i * 7) & 63]
  counts[k] = (counts[k] ?? 0) + 1
  if (i === 500) delete counts[keys[3]]
}
let total = 0
for (const k of Object.keys(counts)) total = total + counts[k]
console.log(total, Object.keys(counts).length, counts[keys[3]], counts[keys[7]])
