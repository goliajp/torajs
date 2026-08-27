// 508-03 — a prototype level lists the methods its class declares.
//
// The class-methods table merges parent chains, because it is the
// dispatch resolution: one lookup has to answer "which body does
// `c.m()` reach". Reifying that merged table as own entries of
// `__proto_<C>` put every inherited method on every subclass
// prototype — a name `hasOwnProperty` reported and
// `getOwnPropertyNames` listed, at a level the spec says has only
// its own. The subclass prototype's [[Prototype]] already points at
// the parent's, so the walk finds the inherited ones without a copy.
class Base {
  m() { return "base.m" }
  shared() { return "base.shared" }
}
class D extends Base {
  own() { return "d.own" }
  shared() { return "d.shared" }
}

console.log(Object.getPrototypeOf(D.prototype) === Base.prototype)
console.log(Object.getOwnPropertyNames(Base.prototype).join(","))
console.log(Object.getOwnPropertyNames(D.prototype).join(","))
console.log(Object.prototype.hasOwnProperty.call(D.prototype, "m"))
console.log(Object.prototype.hasOwnProperty.call(D.prototype, "own"))

// Dispatch is unchanged: the inherited body is still reached, and an
// override still shadows it.
const d = new D()
console.log(d.m(), d.own(), d.shared())
const a: any = d
console.log(a.m(), a.shared())

// Three levels, so the middle one's own set is exactly its own.
class E extends D {
  extra() { return "e.extra" }
}
console.log(Object.getOwnPropertyNames(E.prototype).join(","))
console.log(new E().m(), new E().shared(), new E().extra())
