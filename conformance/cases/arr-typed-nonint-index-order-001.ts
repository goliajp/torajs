// A typed array read through a runtime f64 index splits on whether the
// index is integral before it bounds-checks: a non-integral index names
// a property (§10.4.2, CanonicalNumericIndexString), so 1.5 and -0.5
// read undefined, NaN reads the "NaN" property and -0 reads element 0
// (ToPropertyKey(-0) is "0"), while an integral index is compared with
// the length. Each index comes from argv so the split is decided at run
// time.

const xs: number[] = [10, 20, 30, 40]
const base = process.argv.length - 2
const idxs: number[] = [base + 1.5, base - 0.5, base / base, base + 1, base - 1, base + 4, base + 3, base * 0, base * -1]
for (const i of idxs) {
  console.log(i, xs[i])
}
const words: string[] = ["a", "b", "c"]
let hits = 0
for (let k = 0; k < 12; k = k + 1) {
  const v = words[base + k * 0.5]
  if (v !== undefined) hits = hits + 1
  console.log(k, v)
}
console.log(hits)
