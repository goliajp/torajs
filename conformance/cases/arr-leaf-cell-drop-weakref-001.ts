// an array of scalar-field class cells released in one walk while some
// elements are WeakRef targets: each observed element is cleared when it
// dies, the rest are freed around it, and a later allocation reusing the
// memory never shows through deref. WeakRef timing is not specified, so
// a deref may answer undefined or the live element; a dangling deref
// (garbage fields) is the failure this pins.
class Leaf {
  a: number
  b: number
  constructor(a: number, b: number) {
    this.a = a
    this.b = b
  }
}
const n = Number(process.argv[2] ?? "300")
let xs: Leaf[] = []
for (let i = 0; i < n; i = i + 1) {
  xs.push(new Leaf(i, i * 2))
}
const refs: WeakRef<Leaf>[] = []
for (let i = 0; i < n; i = i + 37) {
  refs.push(new WeakRef(xs[i]))
}
const kept = xs[74]
xs = []
let ys: Leaf[] = []
for (let i = 0; i < n * 2; i = i + 1) {
  ys.push(new Leaf(-1, -1))
}
let ok = true
for (let k = 0; k < refs.length; k = k + 1) {
  const d = refs[k].deref()
  if (d !== undefined && (d.a !== k * 37 || d.b !== k * 74)) ok = false
}
console.log(ok, refs.length, kept.a, kept.b, ys.length)
const live = refs[2].deref()
console.log(live === kept, live === undefined ? -1 : live.b)
