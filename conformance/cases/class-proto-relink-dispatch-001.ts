// 508-03 — after a prototype is re-linked, the chain answers.
//
// The class-methods table is a merged, compile-time answer to "what
// would the prototype chain say". It stops being one the moment a
// link moves: `setPrototypeOf(D.prototype, standin)` puts `standin`
// ahead of `Base`, and the table still named `Base`'s body. The
// shortcut is now gated on whether a re-link has actually happened —
// a runtime fact, so a program that never re-links pays nothing and
// one that does gets the live walk.
class Base {
  m() { return "base.m" }
  kept() { return "base.kept" }
}
class D extends Base {}

const standin = {
  m() { return "standin.m" },
  only() { return "standin.only" },
}

const before: any = new D()
console.log(before.m(), before.kept())

Object.setPrototypeOf(D.prototype, standin)

const after: any = new D()
console.log(after.m())
console.log(after.only())
console.log(typeof after.kept)
console.log(Object.getPrototypeOf(D.prototype) === standin)

// An unrelated class is untouched by someone else's re-link.
class Other {
  m() { return "other.m" }
}
const o: any = new Other()
console.log(o.m())
