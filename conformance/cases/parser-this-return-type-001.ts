// V3-18 wedge — `this` as a return-type annotation (TS
// polymorphic-this, spec §3.6.3). The dominant TS pattern this
// unblocks is fluent builder APIs where every method returns
// `this` to chain:
//   class Builder { add(s): this { ...; return this } }
// Pre-fix tora's `parse_type_ann` bailed at
// 'expected type name, got This' the moment it saw the
// `this` keyword in any type-ann position.
//
// Implementation:
// * parser.rs adds Token::This to the name-acceptors in
//   parse_type_ann, storing the literal `"this"` string in
//   the flat ann.
// * ast::desugar_classes rewrites every method's
//   `return_type` through `rewrite_this_in_ann`, substituting
//   the placeholder for the enclosing class's this_ann (e.g.
//   `C` or `C<T|U>` for generic classes). check.rs and
//   ssa_lower then see the concrete class type at every
//   method's return boundary.
// * Also handles `__nullable(this)` for the rare
//   `: this | null` shape.
//
// Outside class bodies the placeholder leaks through to
// typecheck and is rejected — matches TS spec which only
// allows `this` types inside class bodies.

// Fluent builder — the canonical use.
class Builder {
  parts: string[] = []
  add(s: string): this { this.parts.push(s); return this }
  addLine(s: string): this { this.parts.push(s + "\n"); return this }
  build(): string { return this.parts.join("") }
}
let b = new Builder()
console.log(b.add("a").add("b").addLine("c").build())
// abc + \n at the end (toString collapses trailing newline)

// Numeric counter with multiple chainable mutators.
class Counter {
  n: number = 0
  inc(): this { this.n++; return this }
  dec(): this { this.n--; return this }
  get(): number { return this.n }
}
let c = new Counter()
console.log(c.inc().inc().inc().dec().get())   // 2

// Inheritance: `this` in the base method, called through the
// subclass instance, returns the subclass type at the source
// level. At the desugar level both ends share the same flat
// class chain so the dispatch picks the inherited method.
class ExtCounter extends Counter {
  twice(): this { this.inc(); return this.inc() }
}
let e = new ExtCounter()
console.log(e.twice().twice().get())           // 4

// Mixed chain — base + subclass method on the same builder.
e.dec()
console.log(e.get())                           // 3
