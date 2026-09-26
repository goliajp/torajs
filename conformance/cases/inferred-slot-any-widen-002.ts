// The return type sniffed from a reassigned binding: a binding a
// later assign gives an `any` value returns that value as is.

function f(a: any) {
  let t = 0
  t = t + a
  return t
}
console.log(f('x'), typeof f('x'), f(2), typeof f(2))

function g(a: any) {
  let t = 0
  t = t + 1
  return t
}
console.log(g(1), typeof g(1))

function h(a: any) {
  let t = 0
  let u = 1
  t = t + a
  u = t
  return u
}
console.log(h('y'), typeof h('y'))

function count(n: number) {
  let i = 0
  while (i < n) i = i + 1
  return i
}
console.log(count(5) * 2)

function pick(flag: boolean, v: any) {
  let r = 'none'
  if (flag) r = v
  return r
}
console.log(pick(true, 42), typeof pick(true, 42), pick(false, 42))
