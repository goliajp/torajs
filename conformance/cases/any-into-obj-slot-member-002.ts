// An object literal laid out at a declared type casts an `any` member
// into that member's object slot wherever the literal is written: an
// argument to a parameter of the type, a value assigned to a binding
// annotated with it (a local, a module global, one a closure
// captures), a field a constructor initializes. When that cast is the
// only raise in a function, the caller still checks for it, so the
// TypeError reaches the handler around the call. bun stores the value
// as it is (the lines that differ are checked against the .expected).
class X {
  n = 5
}
type T = { s: string }
type Q = { t: T }

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message)
  }
}
function take(q: Q): number {
  return 1
}
function arg(v: any): number {
  return take({ t: v })
}
function assignLocal(v: any): number {
  let q: Q = { t: { s: "a" } }
  q = { t: v }
  return 2
}
let g: Q = { t: { s: "g" } }
function assignGlobal(v: any): number {
  g = { t: v }
  return 3
}
function assignCaptured(v: any): number {
  let q: Q = { t: { s: "c" } }
  const set = (): void => {
    q = { t: v }
  }
  set()
  return 4
}
type N = { q: Q }
function nested(v: any): number {
  return take2({ q: { t: v } })
}
function take2(n: N): number {
  return 5
}
class C {
  q: Q
  constructor(v: any) {
    this.q = { t: v }
  }
}
function ctor(v: any): number {
  new C(v)
  return 6
}
const x: any = new X()
attempt("arg", () => console.log(arg(x)))
attempt("assign local", () => console.log(assignLocal(x)))
attempt("assign global", () => console.log(assignGlobal(x)))
attempt("assign captured", () => console.log(assignCaptured(x)))
attempt("nested arg", () => console.log(nested(x)))
attempt("ctor field", () => console.log(ctor(x)))
attempt("arg fields", () => console.log(arg({ s: "k" })))
attempt("assign global fields", () => console.log(assignGlobal({ s: "k" })))
console.log(typeof g.t.s)
