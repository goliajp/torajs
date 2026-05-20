// P8.5 — class expressions as values, anonymous form.
// Pre-A1, every shape errored at the parser layer
// (`parse error: expected expression, got Class`). After A1, the
// expression-position class is buffered as a synthesized ClassDecl
// (`__ClassExpr_<id>`) and the use site receives an Ident that
// `synthesize_class_globals` exposes through the standard
// class-as-value substrate. `new F()` rewrites at parse time via the
// narrow const-binding alias map to dispatch through the static
// factory `__new___ClassExpr_<id>`.
//
// NOTE: each anonymous class uses a distinct method name. Three or
// more classes sharing the same method name surfaces a pre-existing
// dispatch corruption in tora's class-method-by-name resolution
// (independent of P8.5; reproducible with literal top-level
// `class A { tag() {...} } class B { tag() {...} } class C { tag()
// {...} }` — observed `a c c` instead of `a b c`). Parked as an
// L3b follow-up.

// 1) Simple anonymous class with one method.
const F1 = class {
  greet(): string { return "F1.greet" }
}
console.log(new F1().greet())

// 2) Anonymous class with constructor + field + method.
const F2 = class {
  x: number
  constructor(n: number) { this.x = n }
  double(): number { return this.x * 2 }
}
const f2 = new F2(7)
console.log(f2.x, f2.double())

// 3) Two anonymous class expressions in the same scope, each with
//    its own distinctly-named method so the multi-class dispatch
//    bug doesn't mask the synth-id distinction.
const F3a = class {
  alpha(): string { return "from-alpha" }
}
const F3b = class {
  beta(): string { return "from-beta" }
}
console.log(new F3a().alpha(), new F3b().beta())

// 4) Alias chain: const G = F propagates the alias so `new G()`
//    rewrites to the same synth factory as `new F()`.
const F4 = class {
  msg(): string { return "via alias" }
}
const G4 = F4
console.log(new G4().msg())

// 5) Anonymous class with multiple methods + cross-method call.
const F5 = class {
  base: number
  constructor(n: number) { this.base = n }
  inc(): number { return this.base + 1 }
  thrice(): number { return this.inc() + this.inc() + this.inc() }
}
console.log(new F5(10).thrice())
