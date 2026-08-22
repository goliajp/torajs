// The same self-referential `any` binding, declared inside a block and
// inside a function body rather than at the top level. A top-level
// `let` becomes a global slot, which the capture walk resolves through
// `globals`; a block-scoped one has to be pre-declared by the checker
// before the init types, and that gate declined an `any` annotation
// in step with a lowering lane that has since been widened.
{
  let g: any = function () { return typeof g; };
  console.log("block:", g());
}

function outer() {
  let k: any = function (n: number): number { return n <= 1 ? 1 : n * k(n - 1); };
  console.log("fn-scope:", k(5));
}
outer();

// Reading the binding itself from inside its own body, not just its
// `typeof` — `typeof` on an unresolved name is legal and answers
// "undefined", so it alone would not have shown the gap.
{
  let s: any = function () { return s; };
  console.log("returns-self:", typeof s());
}
