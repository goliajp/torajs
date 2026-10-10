function mk(k: number): (x: number) => number {
  return (x: number) => x + k
}
const shared = (x: number) => x * 2
function get(): (x: number) => number {
  return shared
}
function pick(c: boolean): (x: number) => number {
  return c ? shared : mk(1)
}
let total = 0
const n = 20000
for (let i = 0; i < n; i = i + 1) {
  total = total + mk(i)(1) + get()(i) + pick(i % 2 === 0)(i)
}
console.log(total, shared(21), get()(4))
