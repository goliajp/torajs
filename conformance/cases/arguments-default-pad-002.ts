// A defaulted parameter never changes `arguments.length`: it counts the
// arguments the call passed, whichever lane reaches the function.
const k: any = function (a: number, b = 5) {
  return arguments.length
}
console.log(k(1), k(), k(1, 2, 3))

const h: any = function (a: number, b: number) {
  return arguments.length
}
console.log(h(1), h())

const q: any = function (a: number, b: number = 5) {
  return arguments.length + ':' + arguments[0]
}
console.log(q(1), q())

const f = function (a: number, b: number = 5) {
  return arguments.length
}
console.log(f(1), f(1, 2))
const fs = [f]
console.log(fs.length, fs[0](1))

const r = function (a: number, b: string = 'x') {
  return arguments.length + b
}
console.log(r(1), r(1, 'y'))
const rs = [r]
console.log(rs[0](1))

const d = function (a: number, b: number = 5) {
  return arguments.length * 10 + b
}
const ds = [d]
console.log(d(1), d(1, 2), ds[0](1), d(1, undefined))

const o = {
  m(a: number, b = 5) {
    return arguments.length
  },
}
console.log(o.m(1), o.m(1, 2))
const oa: any = o
console.log(oa.m(1), oa.m(1, 2))

function g(a: number, b = 5) {
  return arguments.length
}
console.log(g(1), g(1, 2))
const gv: any = g
console.log(gv(1), gv(1, 2, 3))

const v = function (a: number, b = 5, ...r: number[]) {
  return arguments.length + ':' + b + ':' + r.length
}
console.log(v(1), v(1, 2), v(1, 2, 3, 4))
const vs = [v]
const ov = {
  v(a: number, b = 5, ...r: number[]) {
    return arguments.length + ':' + b + ':' + r.length
  },
}
console.log(ov.v(1), ov.v(1, 2, 3))
