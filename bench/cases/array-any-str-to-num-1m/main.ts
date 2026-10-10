// any[] of numeric strings converted with unary plus every iteration:
// the is-number guard in front of ToNumber fails each time, so the
// string-to-number call is the path that runs.
const n = parseInt(process.argv[2], 10)
const pool = parseInt(process.argv[3], 10)
const arr: any[] = []
for (let j = 0; j < pool; j = j + 1) arr.push('' + j)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const s = arr[i % pool]
  total = total + +s
}
console.log(total)
