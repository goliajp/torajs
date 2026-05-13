// V3-18 wedge — every class always emits a `__cm_<C>__ctor`
// symbol, even when the user wrote no explicit `constructor`,
// per ES spec §15.7.10 ("default constructor"). Pre-fix tora
// only emitted ctor when the user declared one — so any
// subclass calling `super()` against a parent without an
// explicit ctor failed typecheck with
// 'unknown identifier __cm_<Parent>__ctor'.
//
// Implementation: in ast::desugar_classes the ctor-emit
// branch is unconditional. When ctor is None the body is
// empty and the param list is just `__this`. The factory
// path (build_factory_body) still gates `__cm_C__ctor(__this)`
// call on `ctor.is_some()`, so the no-ctor case adds an
// unreferenced empty fn for ctor-less classes — observable
// only via `super()` from a subclass.

// Direct super() to a parent with no explicit ctor.
class A {
  hello(): string { return "from A" }
}
class B extends A {
  constructor() { super() }
  bee(): string { return "B" }
}
let b = new B()
console.log(b.hello())                 // from A
console.log(b.bee())                   // B

// Chained super() through a parent that itself has no ctor.
class Pet {
  pname: string = "unknown"
  intro(): string { return "Hi from " + this.pname }
}
class Doggy extends Pet {
  constructor() { super(); this.pname = "Rex" }
}
class Pup extends Doggy {
  constructor() { super(); this.pname = "Junior" }
}
let d = new Doggy()
console.log(d.intro())                 // Hi from Rex
let p = new Pup()
console.log(p.intro())                 // Hi from Junior

// Three-deep chain where the top of the chain has no ctor.
class Top {
  topLabel(): string { return "top" }
}
class MidLayer extends Top {
  constructor() { super() }
}
class SubLayer extends MidLayer {
  constructor() { super() }
  via(): string { return this.topLabel() + "/sub" }
}
let s = new SubLayer()
console.log(s.via())                   // top/sub

// Sanity: a class with no ctor and no subclasses still
// behaves the same — factory does NOT call the empty ctor
// (the elision in build_factory_body is preserved).
class Plain {
  v: number = 7
}
let plain = new Plain()
console.log(plain.v)                   // 7
