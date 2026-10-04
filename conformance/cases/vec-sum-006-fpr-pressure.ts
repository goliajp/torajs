// A widened sum with fourteen float values live across it: more
// values than the SIMD&FP registers hold together with the loop's
// vectors, so some must spill, and none of them may be a vector.
const n = parseInt(process.argv[2] ?? '1000', 10)
const f0 = parseFloat(process.argv[3] ?? '0.15')
const f1 = parseFloat(process.argv[4] ?? '1.25')
const f2 = parseFloat(process.argv[5] ?? '2.35')
const f3 = parseFloat(process.argv[6] ?? '3.45')
const f4 = parseFloat(process.argv[7] ?? '4.55')
const f5 = parseFloat(process.argv[8] ?? '5.65')
const f6 = parseFloat(process.argv[9] ?? '6.75')
const f7 = parseFloat(process.argv[10] ?? '7.85')
const f8 = parseFloat(process.argv[11] ?? '8.95')
const f9 = parseFloat(process.argv[12] ?? '9.105')
const f10 = parseFloat(process.argv[13] ?? '10.115')
const f11 = parseFloat(process.argv[14] ?? '11.125')
const f12 = parseFloat(process.argv[15] ?? '12.135')
const f13 = parseFloat(process.argv[16] ?? '13.145')
const xs: number[] = []
for (let i = 0; i < n; i = i + 1) {
  xs.push(i)
}
let s = 0
for (let i = 0; i < xs.length; i = i + 1) {
  s = s + xs[i]
}
console.log(s)
console.log(f0 * 1 + f1 * 2 + f2 * 3 + f3 * 4 + f4 * 5 + f5 * 6 + f6 * 7)
console.log(f7 * 8 + f8 * 9 + f9 * 10 + f10 * 11 + f11 * 12 + f12 * 13 + f13 * 14)
