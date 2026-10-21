// The same binding inside a function, captured by closures, and read
// as a global by a named function: each holds the `any` value as is.

const str: any = 'str'
const num: any = 41

function f(a: any) {
  const t: number = a
  return typeof t + ':' + (t + 1)
}
console.log(f('x'), f(3))

function g(a: any): number {
  let u: number = a
  u++
  return u
}
console.log(g('x'), g(41))

const c: number = str
const fns = [() => c + '!', () => typeof c]
console.log(fns[0](), fns[1]())

let w: number = num
w += 1
console.log(w, w === 42, typeof w)

function readC() {
  return c + 1
}
console.log(readC())

function id<T>(v: T): T {
  const h: any = v
  const k: string = h
  return k as any
}
console.log(id(5), id('q'))

let total = 0
const ys: any[] = [1, 2, 3]
for (let i = 0; i < ys.length; i++) {
  const v: number = ys[i]
  total = total + v
}
console.log(total, typeof total)

const zs: any[] = [1, '2', 3]
let joined = 0
for (let i = 0; i < zs.length; i++) {
  const v: number = zs[i]
  joined = joined + v
}
console.log(joined, typeof joined)

// a method's inferred return reads the binding, not its annotation
class K {
  f(a: any) {
    const z: boolean = a
    return z
  }
}
console.log(new K().f(0), new K().f('s'))

// a top-level `var`, read and written by named functions
var vv: number = str
function readV() {
  return vv
}
console.log(readV(), typeof readV())
vv = 3
console.log(readV() + 1)
let gl: string = str
function setG(x: string) {
  gl = x
}
setG('new')
console.log(gl, typeof gl)
