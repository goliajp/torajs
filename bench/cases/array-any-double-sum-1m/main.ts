// array-any-indexed-100k with fractional elements: the any accumulator
// holds a double from the first iteration, so the generic number arm
// of `total + x` runs every time instead of the int fast path.
const n = parseInt(process.argv[2], 10)
const pool = parseInt(process.argv[3], 10)
const arr: any[] = []
for (let j = 0; j < pool; j = j + 1) arr.push(j + 0.5)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const x = arr[i % pool]
  total = total + x
}
console.log(total)
