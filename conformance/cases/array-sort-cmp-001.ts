// Perf Round 5 attack #1 (RFC 20260703-perf-arr-sort-nlogn) — user-
// comparator sort routed through the __torajs_arr_sort_cb runtime
// helper (stable O(n log n) merge sort + insertion base, comparator
// called back through the closure ABI). Pre-fix the SSA-emitted
// inline insertion sort did 243,901 comparator calls for a 1000-
// element random array where JSC's merge sort does 8,686.
//
// Covers the helper's dispatch matrix:
// * i64 elements + i64-returning arrow comparator (bench shape)
// * descending comparator
// * f64-returning comparator (a - b on number stays I64 here; the
//   f64 case is exercised via fractional elements)
// * closure comparator capturing an outer variable (env pointer path)
// * stability: equal-key objects keep insertion order — observed
//   via a parallel index array sorted by key only
// * sort on a 1000-element LCG array (crosses the insertion-run
//   cutoff so the merge path actually runs) — checksum must match
//   bun exactly
// * toSorted with comparator (clone-then-sort)
// * comparator throw propagates and leaves the array intact-length

// basic ascending / descending
let xs = [170, 45, 75, 90, 802, 24, 2, 66]
xs.sort((a: number, b: number) => a - b)
console.log(xs)                        // [ 2, 24, 45, 66, 75, 90, 170, 802 ]
xs.sort((a: number, b: number) => b - a)
console.log(xs)                        // [ 802, 170, 90, 75, 66, 45, 24, 2 ]

// fractional (f64) elements
let fs = [3.5, -1.25, 0.5, 99.125, 2.5, -7.75]
fs.sort((a: number, b: number) => a - b)
console.log(fs)                        // [ -7.75, -1.25, 0.5, 2.5, 3.5, 99.125 ]

// closure comparator capturing an outer flag (env path)
let dir = -1
let cap = [5, 3, 9, 1, 7]
cap.sort((a: number, b: number) => (a - b) * dir)
console.log(cap)                       // [ 9, 7, 5, 3, 1 ]

// stability probe: keys with ties; a second array records original
// positions and must come out grouped in insertion order per key.
let keys = [2, 1, 2, 1, 2, 1, 2, 1]
let tagged: number[] = []
for (let i = 0; i < keys.length; i = i + 1) {
  tagged.push(keys[i] * 100 + i)
}
tagged.sort((a: number, b: number) => ((a / 100) | 0) - ((b / 100) | 0))
console.log(tagged)                    // [ 101, 103, 105, 107, 200, 202, 204, 206 ]

// 1000-element LCG array — forces the merge path (> insertion run)
function build(n: number, seed: number): number[] {
  const out: number[] = []
  let s = seed | 0
  for (let i = 0; i < n; i = i + 1) {
    s = ((s * 48271) | 0) & 0x7fffffff
    if (s === 0) s = 1
    out.push(s)
  }
  return out
}
const big = build(1000, 42)
big.sort((a: number, b: number) => a - b)
let ordered = true
for (let i = 1; i < 1000; i = i + 1) {
  if (big[i - 1] > big[i]) ordered = false
}
console.log(ordered)                   // true
console.log(big[0] + big[500] + big[999])

// toSorted with comparator — source stays intact
const src = [30, 10, 20]
const sorted = src.toSorted((a: number, b: number) => a - b)
console.log(src)                       // [ 30, 10, 20 ]
console.log(sorted)                    // [ 10, 20, 30 ]

// comparator throw propagates; array keeps its length
let boom = [4, 2, 7, 5, 1, 9, 8, 3]
let caught = ""
try {
  boom.sort((a: number, b: number) => {
    if (a === 7 || b === 7) throw new Error("cmp boom")
    return a - b
  })
} catch (e: any) {
  caught = e.message
}
console.log(caught)                    // cmp boom
console.log(boom.length)               // 8
