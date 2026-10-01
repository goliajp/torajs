// a sparse tail: the indices past the materialized extent are absent,
// the callback never sees them and the product keeps the length
const a: any[] = [1, 2]
a.length = 33554500
let calls = 0
const b = a.map((v: any) => {
  calls = calls + 1
  return v * 3
})
console.log(calls, b.length, 0 in b, 1 in b, 2 in b, 33554499 in b, b[1])
