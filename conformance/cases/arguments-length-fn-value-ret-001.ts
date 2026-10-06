// A function value whose body returns `arguments.length` returns a
// number, so a typed callback slot reads the count, not a raw box.
const cb = (fn: (x: number) => number) => fn(9)
const cb2 = (fn: (x: number, y: number) => number) => fn(9, 1) + 1

const f = function (a: number, b: number = 5) {
  return arguments.length
}
console.log(cb(f))

const g = function (a: number, b: number = 5) {
  return arguments.length + 0
}
console.log(cb(g))

const p = function (a: number, b: number) {
  return arguments.length
}
console.log(cb2(p))

const w = function (a: number, b: number = 5) {
  arguments.length = 7
  return arguments.length
}
console.log(cb(w))
