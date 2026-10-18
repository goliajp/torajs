// An object literal element of an array literal lowered against an
// array type is laid out at the declared element type, as an object
// literal lowered against an object type is: every reader of the array
// reads its elements at that layout. An `any` member the element type
// gives an object type is then cast into its slot like the member of
// any declared literal: an object with the fields crosses as a copy,
// and a value its readers could not read as that type (an instance of
// a class without the fields) throws a TypeError. bun stores the
// value as it is and reads `undefined` from it; those lines differ
// from bun and are checked against the .expected.
class X {
  n = 5
}
type S = { s: string }
type T = { ts: { t: S }[] }

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError)
  }
}
function inMember(v: any): string {
  const a: T = { ts: [{ t: v }] }
  return a.ts[0].t.s
}
function inLet(v: any): string {
  const a: { t: S }[] = [{ t: v }, { t: { s: "lit" } }]
  return a[0].t.s + a[1].t.s
}
function nested(v: any): string {
  const a: { t: S }[][] = [[{ t: v }]]
  return a[0][0].t.s
}
function returned(v: any): { t: S }[] {
  return [{ t: v }]
}
let held: { t: S }[] = []
class Box {
  ts: { t: S }[] = []
}
function stored(v: any): string {
  const b = new Box()
  b.ts = [{ t: v }]
  return b.ts[0].t.s
}
function assigned(v: any): string {
  held = [{ t: v }]
  return held[0].t.s
}

const good: any = { s: "ok" }
attempt("member", () => console.log(inMember(good)))
attempt("let", () => console.log(inLet(good)))
attempt("nested", () => console.log(nested(good)))
attempt("return", () => console.log(returned(good)[0].t.s))
attempt("assign", () => console.log(assigned(good)))
attempt("store", () => console.log(stored(good)))
attempt("member instance", () => console.log(typeof inMember(new X())))
attempt("let instance", () => console.log(typeof inLet(new X())))
attempt("nested instance", () => console.log(typeof nested(new X())))
attempt("return instance", () => console.log(typeof returned(new X())[0].t.s))
attempt("store instance", () => console.log(typeof stored(new X())))
