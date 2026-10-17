// An `any` value written into a slot whose declared type is an object
// layout crosses the same way wherever the slot is: an object literal's
// member, a field written through a receiver the program annotated, a
// returned value. A cell the slot's readers can read as it is (one of
// the layout, or of a layout that extends it field for field) keeps
// its identity; an object that is no class instance and has the fields
// is copied; anything else throws a TypeError at the write, as a `let`
// of a class does. bun stores every one of them as it is (the lines
// that differ are checked against the .expected): the slot is read and
// released as the layout it declares, so a cell of another layout in
// it would be read at the wrong offsets.
class X {
  n = 5
}
class W {
  a = 1
  b = 2
}
class W2 {
  a = 7
  b = 8
  c = "c"
}
type T = { s: string }
type Q = { t: T }
type H = { t: T }
type R = { w: W }

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message)
  }
}
function lit(v: any): string {
  const q: Q = { t: v }
  return typeof q.t.s
}
function store(v: any): string {
  const h: H = { t: { s: "a" } }
  h.t = v
  return typeof h.t.s
}
function ret(v: any): T {
  return v
}
function litW(v: any): number {
  const r: R = { w: v }
  return r.w.a + r.w.b
}
// the only raise in this body is the member's cast
function dropOnly(v: any): number {
  const q: Q = { t: v }
  return 1
}
const x: any = new X()
attempt("member", () => console.log(lit(x)))
attempt("store", () => console.log(store(x)))
attempt("return", () => console.log(typeof ret(x).s))
attempt("unread member", () => console.log(dropOnly(x)))
attempt("member number", () => console.log(lit(3)))
attempt("member missing field", () => console.log(lit({ u: 1 })))
attempt("member fields", () => console.log(lit(JSON.parse('{"s":"j"}'))))
attempt("member wider class", () => console.log(litW(new W2())))
attempt("member class", () => console.log(litW(new W())))
attempt("store fields", () => console.log(store({ s: "k" })))
attempt("unread member fields", () => console.log(dropOnly({ s: "k" })))
