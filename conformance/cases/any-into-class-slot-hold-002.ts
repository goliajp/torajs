// A binding annotated with a class holds an `any` value as it is, so a
// store through a setter the class declares runs whatever the value
// does with the name (§10.1.9 OrdinarySet: the class's setter on an
// instance, another class's setter, a plain data property on an
// object, a TypeError on a primitive in strict code), and a read of a
// private name the class declares throws a TypeError when the value has
// no such field (§7.3.31 PrivateGet), inside the class body as anywhere.
class C {
  x = 1
  #p = 5
  set s(v: number) { this.x = v * 2 }
  static readP(v: any) { const q: C = v; return q.#p }
  static hasP(v: any) { const q: C = v; return #p in q }
}
class D {
  x = 10
  set s(v: number) { this.x = v + 100 }
}
function attempt(tag: string, f: () => unknown) {
  try { console.log(tag, f()) } catch (e) { console.log(tag, "threw", (e as Error).constructor.name) }
}
function store(v: any, n: number) { const q: C = v; q.s = n; return [q.x, Object.keys(q).join(",")].join(" ") }
for (const v of [new C(), new D(), { x: 3 }, 5, undefined]) {
  attempt("store", () => store(v, 7))
  attempt("readP", () => C.readP(v))
}
for (const v of [new C(), new D(), { x: 3 }]) {
  attempt("hasP", () => C.hasP(v))
}
