// the same key cells read and written across several number
// dictionaries whose entries sit at different indices, through deletes,
// re-adds, growth and non-number values stored through an alias
type Rec = { key: string; w: number }

const recs: Rec[] = []
for (let i = 0; i < 40; i = i + 1) {
  recs.push({ key: "q" + i, w: i % 7 })
}
const a: Record<string, number> = {}
const b: Record<string, number> = {}
for (let i = recs.length - 1; i >= 0; i = i - 1) {
  b[recs[i].key] = 100 + i
}
for (let round = 0; round < 3; round = round + 1) {
  for (let i = 0; i < recs.length; i = i + 1) {
    const r = recs[i]
    a[r.key] = (a[r.key] ?? 0) + r.w
    b[r.key] = (b[r.key] ?? 0) + 1
  }
}
let sa = 0
let sb = 0
for (const r of recs) {
  sa = sa + a[r.key]
  sb = sb + b[r.key]
}
console.log(sa, sb, Object.keys(a).length, Object.keys(b).length)

// delete then re-add: the re-added entry lands at a new index
for (let i = 0; i < recs.length; i = i + 3) {
  delete a[recs[i].key]
}
console.log(Object.keys(a).length, a[recs[0].key], a[recs[1].key])
for (let i = 0; i < recs.length; i = i + 1) {
  const r = recs[i]
  a[r.key] = (a[r.key] ?? 0) + 1
}
console.log(Object.keys(a).join(",").length, a[recs[0].key], a[recs[1].key], a[recs[3].key])

// growth rebuilds the store under the remembered indices
const g: Record<string, number> = {}
const k0 = recs[5].key
g[k0] = 1
for (let i = 0; i < 300; i = i + 1) {
  g["g" + i] = i
  g[k0] = (g[k0] ?? 0) + 1
}
console.log(g[k0], g["g299"], Object.keys(g).length)

// a non-number stored through an alias, then read through the typed lane
const alias: any = a
alias[recs[2].key] = "s"
const v = a[recs[2].key]
console.log(typeof v, v)
a[recs[2].key] = (a[recs[2].key] ?? 0) + 1
console.log(a[recs[2].key])
alias[recs[4].key] = null
a[recs[4].key] = (a[recs[4].key] ?? 10) + 1
console.log(a[recs[4].key])

// an inherited name and a key the dictionary never had
const p: Record<string, number> = {}
console.log(p[recs[6].key] ?? -1, typeof p["toString"])
