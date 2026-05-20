// P8.5 — `new (class { ... })()` IIFE-style immediate construction.
// Pre-A1, parse_new only accepted Token::Ident as the new target. After
// A1, two new arms cover the parenthesized-callee shape:
//   (a) Token::Class directly after `new` → inline class expression
//   (b) Token::LParen after `new` → parses `(expr)`, accepts Ident inner
// Path (b) is what `new (class { ... })()` exercises — parse_primary's
// Class branch emits an Ident at the inner site, which the LParen arm
// then accepts as the class name.
//
// NOTE: cases involving a subclass without its own constructor when
// the parent has constructor args are intentionally excluded — that
// surfaces a pre-existing tora limitation (default ctor synthesis
// ignores parent's arity, typecheck reports "expected 0 argument(s),
// got 1"). Reproducible with literal top-level subclass form too;
// parked as an L3b follow-up.

// 1) Bare immediate-new: anonymous class, no extends, no args.
const r1 = new (class {
  v(): number { return 1 }
})()
console.log(r1.v())

// 2) Immediate-new with constructor args.
const r2 = new (class {
  msg: string
  constructor(s: string) { this.msg = s }
  show(): string { return this.msg + "!" }
})("hello")
console.log(r2.show())

// 3) `new class { ... }(args)` form (Token::Class arm of parse_new,
//    no parens around the class). Subclass without own ctor + parent
//    with no-arg ctor is safe — avoids the pre-existing inherited-
//    ctor-arity limitation.
class Base3 {
  base(): string { return "base" }
}
const r3 = new class extends Base3 {
  child(): string { return "child" }
}()
console.log(r3.base(), r3.child())

// 4) Immediate-new with extends, subclass declares its OWN ctor
//    threading the arg into the parent. Avoids the inherited-ctor
//    issue from case 4 of the original draft.
class P4 {
  tag: string
  constructor(t: string) { this.tag = t }
}
const r4 = new (class extends P4 {
  constructor(t: string) { super(t) }
  upper(): string { return this.tag.toUpperCase() }
})("pq")
console.log(r4.tag, r4.upper())

// 5) Immediate-new result passed through a function call. Exercises
//    that the instance is a normal object value, not parser-specific
//    magic.
function announce(s: string): void {
  console.log("instance says " + s)
}
const r5 = new (class {
  word(): string { return "hi" }
})()
announce(r5.word())
