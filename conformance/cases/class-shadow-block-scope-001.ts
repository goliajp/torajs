// `class_globals_shadow` rewrites a value-position `Ident(C)` to the
// class-object sentinel, and must not do so where a local binding owns
// the name. Its census collected declarations "at fn-level
// granularity" on purpose, and the module doc argued the coarseness
// was safe: a deep block-scoped `let R` hiding `R` for the whole body
// "can only turn a would-be-rewritten reference into a LOUD unknown
// identifier, never a silent wrong."
//
// `typeof` is the counter-example. It does not throw, so the coarse
// answer came back as a plain wrong value.

class W {
  tag: number = 1;
}

// The name is rebound in a nested block; after the block it is the
// class again — `typeof` says so without throwing either way.
function afterBlock(): void {
  {
    const W: any = 5;
    console.log("inside", W + 1);
  }
  console.log("after", typeof W);
  const x: any = new W();
  console.log("after-new", x.constructor === W, x.tag);
}
afterBlock();

// Reading it before the block is unaffected.
function beforeBlock(): void {
  console.log("before", typeof W);
  {
    const W: any = 5;
    console.log("inside2", W + 1);
  }
}
beforeBlock();

// What the pass exists for must not regress. A PARAMETER rebinds the
// name for the whole body (rotation 428: `R + 1` added 1 to the class's
// toString instead of to the parameter).
class R {}
function helper(R: any): any {
  return R + 1;
}
console.log("param", helper(41));

// A body-top-level `const` is the function's own scope, so it still
// hides the class for the whole body.
class S {
  tag: number = 1;
}
function bodyConst(): void {
  const S: any = 7;
  console.log("body", S + 1);
}
bodyConst();

// A `var` is function-scoped wherever it is written, so a `var` in a
// nested block still hides the name for the whole body.
class V {
  tag: number = 1;
}
function varInBlock(): void {
  {
    var V: any = 9;
  }
  console.log("var", typeof V, V + 1);
}
varInBlock();

// The program scope shadows too — the doc names this one:
// `{ class C {} } { let C = 42 }` must answer the binding.
{
  class C {}
}
{
  const C: any = 42;
  console.log("program", C);
}
