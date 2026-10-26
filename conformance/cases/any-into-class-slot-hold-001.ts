// A binding annotated with a class holds an `any` value as it is: an
// instance of the class, a plain object with or without the class's
// method, a Map handed to a Map subclass's slot, a number, undefined.
// Its members are read off the value itself, so a method the value
// lacks is a TypeError when it is called (§7.3.13 Call), not when the
// value is stored, `instanceof` answers for the value, and a write
// through the binding lands on the value.
class C {
  x = 1
  #p = 5
  m() { return "C" + this.x }
  get g() { return "g" + this.x }
  pv() { return this.#p }
}
class D { x = 2; m() { return "D" + this.x } }
class MP extends Map<string, number> { tag() { return "mp" } }
function attempt(tag: string, f: () => any) {
  try { console.log(tag, f()) } catch (e) { console.log(tag, "threw", (e as Error).constructor.name) }
}
function t1(v: any) { const q: C = v; return q.m() }
function t2(v: any) { const q: C = v; return q?.m() }
function t3(v: any) { const q: C = v; return q.g }
function t4(v: any) { const q: C = v; return q.x }
function t5(v: any) { const q: C = v; return q.pv() }
function t6(v: any) { const q: MP = v; return q.tag() }
function t7(v: any) { const q: MP = v; return q?.tag() }
function t8(v: any) { const q: MP = v; return q.size }
function t9(v: any) { const q: C | undefined = v; return q?.m() }
function t10(v: any) { const q: C = v; const f = q.m; return typeof f }
function t11(v: any) { const q: C = v; return q instanceof C }
function t12(v: any) { const q: C = v; q.x = 9; return v.x }
function t13(v: any) { let q: C = new C(); q = v; return q === v }
function give(v: any): any { return v }
function t14(v: any) { const q: C = give(v); return q.x }
for (const v of [new C(), new D(), { x: 3 }, { x: 4, m() { return "lit" } }, new Map(), new MP(), 5, undefined]) {
  attempt("t1", () => t1(v)); attempt("t2", () => t2(v)); attempt("t3", () => t3(v))
  attempt("t4", () => t4(v)); attempt("t5", () => t5(v)); attempt("t6", () => t6(v))
  attempt("t7", () => t7(v)); attempt("t8", () => t8(v)); attempt("t9", () => t9(v))
  attempt("t10", () => t10(v)); attempt("t11", () => t11(v)); attempt("t12", () => t12(v))
  attempt("t13", () => t13(v)); attempt("t14", () => t14(v))
}
