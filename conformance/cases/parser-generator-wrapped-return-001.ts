// V3-18 wedge — generator return type can use the standard
// TS wrapper-form annotations per spec §3.6.4:
//   function* g(): Generator<T> { ... }
//   function* g(): IterableIterator<T> { ... }
//   function* g(): Iterator<T> { ... }
//   function* g(): Iterable<T> { ... }
// Pre-fix tora's Phase J machinery required a bare yield-type
// ann (`function* g(): T`), and the wrapped forms failed at
// check.rs with 'unknown type `Generator<number>` for field
// `value` of `__step_g`'. Real-world TS code uses the wrapped
// form almost exclusively — bun and tsc both accept all four.
//
// Implementation: in parser.rs, after parsing the return-type
// annotation for a generator function, route it through
// `unwrap_generator_return_ann()`. If the head matches one of
// the four recognized wrappers (`Generator`, `IterableIterator`,
// `Iterator`, `Iterable`), peel the outermost angle brackets
// and take the first type-arg as the yield type. The FnDecl's
// `return_type` field is rewritten to the unwrapped form too,
// so the desugar pipeline downstream sees the canonical bare-T
// shape it was already designed for.
//
// `Generator<T, R, N>` (TS-style three-arg form) keeps just T;
// the Return / Next type args are ignored since the subset
// runtime collapses them.
//
// Subset constraint kept (separate issue): only one generator
// per source file. A pre-existing bug at the iterator-class
// monomorphization layer makes multiple generators in the
// same file alias each other's `__Gen_<name>` class —
// orthogonal to this wedge and untouched by it.

function* gen(): Generator<number> {
  yield 1
  yield 2
  yield 3
}
let g = gen()
console.log(g.next().value)            // 1
console.log(g.next().value)            // 2
console.log(g.next().value)            // 3
console.log(g.next().done)             // true
