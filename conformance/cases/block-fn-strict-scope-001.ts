// §14.2.10 — a `function` declared in a block binds the BLOCK. Annex B
// §B.3.3 adds a second, function-scoped binding, but only in sloppy
// code and only for a plain FunctionDeclaration; this file is a
// module, so none of it applies and nothing here escapes its block.
//
// tr used to answer one verdict for two different questions: "this
// declaration is not block-nested" and "this declaration is
// block-nested in strict code" both said "leave the name alone", and
// the second one meant the opposite.

{ function plain() {} }
console.log(typeof plain);

{ function* gen() {} }
console.log(typeof gen);

{ async function asy() {} }
console.log(typeof asy);

{ async function* asyGen() {} }
console.log(typeof asyGen);

function body(): string {
  { function inner() {} }
  return typeof inner;
}
console.log(body());

function bodyAsync(): string {
  { async function innerAsync() {} }
  return typeof innerAsync;
}
console.log(bodyAsync());

// Inside the block the declaration is perfectly ordinary, and it is
// hoisted within it: a call written above the declaration works.
{
  function twice(n: number): number { return n * 2; }
  console.log(twice(21));
}
{
  console.log(early());
  function early(): string { return "hoisted in its block"; }
}

// Its own recursion still resolves, and a nested block gets a binding
// of its own rather than the outer one.
{
  function fact(n: number): number { return n <= 1 ? 1 : n * fact(n - 1); }
  console.log(fact(5));
}
{
  function which(): string { return "outer block"; }
  {
    function which(): string { return "inner block"; }
    console.log(which());
  }
  console.log(which());
}

// Two plain declarations of one name in one block are legal and the
// last wins (§B.3.3.4), so the block gets ONE binding, not two.
{
  function twin(): string { return "first"; }
  function twin(): string { return "second"; }
  console.log(twin());
}

// A body-level declaration is NOT block-nested and keeps the whole
// body, including the part above it.
console.log(topLevel());
function topLevel(): string { return "body level"; }
