// An element of a typed array whose alias escaped into the `any` world
// is passed to a function declaring a typed parameter: the argument is
// bound as passed, not converted to the parameter type.

const d: number[] = [1, 2, 3]
const x: any = d
x[1] = "s"

function id(v: number): number {
  return v
}
let a = 0
a = a + id(d[1])
console.log(a, typeof id(d[1]))

let b: number = 10
b = b + id(d[1])
console.log(b)

function kind(v: number): string {
  return typeof v
}
console.log(kind(d[0]), kind(d[1]))

function pair(p: number, q: number): string {
  return p + "/" + q
}
console.log(pair(d[1], d[2]), pair(d[0], 5))

let total = 0
function bump(v: number) {
  total = total + v
}
bump(d[0])
bump(d[1])
console.log(total)

function dbl(v: number): number {
  return v * 2
}
console.log(dbl(d[0]), dbl(d[1]), dbl(d[2]))

const flags: boolean[] = [true, false]
const fa: any = flags
fa[0] = 0
function not(v: boolean): boolean {
  return !v
}
function show(v: boolean): string {
  return "" + v
}
console.log(not(flags[0]), show(flags[0]), show(flags[1]))

const names: string[] = ["a", "b"]
const na: any = names
na[1] = 7
function len(s: string): number {
  return s === undefined ? -1 : ("" + s).length
}
function echo(s: string): string {
  return s
}
console.log(echo(names[1]), typeof echo(names[1]), len(names[0]))

let picked = d[1]
console.log(id(picked), typeof id(picked))

// a typed array that never escaped keeps the typed call
const t: number[] = [4, 5]
let u = 0
u = u + id(t[0]) + id(t[1])
console.log(u, typeof u)
