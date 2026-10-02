// An Ident-target assignment in statement position (and in a `for`
// step) leaves exactly one reference in the slot: owned rhs (call /
// literal / binop), borrow rhs (ident / member / index / ternary),
// self-assign, module-level globals, any slots, closure slots. Every
// slot is read back after the churn so a missing stake would read a
// recycled cell.
function add1(v: number): number { return v + 1 }
function mk(n: number): number[] {
  const a: number[] = []
  for (let i = 0; i < n; i = i + 1) a.push(i)
  return a
}

let g: string = "g"
let garr: number[] = [1, 2]
function bumpGlobal(): void {
  g = g + "!"
  garr = mk(3)
}

let p = Promise.resolve(0)
for (let i = 0; i < 5; i = i + 1) {
  p = p.then(add1)
}
console.log(await p)

let s = "a"
for (let i = 0; i < 3; i = i + 1) {
  s = s + "b"
}
console.log(s)

let a = mk(2)
let b = a
b = a
a = mk(4)
console.log(a.length, b.length, b[1])
b = b
console.log(b.length)

const o = { arr: mk(3), s: "x" }
let m = mk(1)
m = o.arr
console.log(m.length, o.arr.length)
const nested: number[][] = [mk(1), mk(2)]
m = nested[1]
console.log(m.length)
const cond = nested.length > 1
m = cond ? a : b
console.log(m.length)

let x: any = { k: 1 }
x = mk(5)
x = s
console.log(x)

let f = (v: number): number => v * 2
let f2 = f
f2 = (v: number): number => v * 3
f = f2
console.log(f(2), f2(2))

bumpGlobal()
bumpGlobal()
console.log(g, garr.length)

let step = "t"
for (let i = 0; i < 3; step = step + "-") {
  i = i + 1
}
console.log(step)

let churn = mk(1)
for (let i = 0; i < 20000; i = i + 1) {
  churn = mk(1)
  s = s + ""
}
console.log(churn.length, s.length, a.length, b.length, m.length)
