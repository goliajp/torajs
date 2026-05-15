// P3.4-followup-A — nested FnDecl inside a block at module-top-level
// (vs the original P3.4 scope which only walked top-level FnDecl
// bodies). annexB §B.3.3 specifies function-statement hoisting from
// inside Blocks even at the global script scope. test262 has many
// `function-statement hoisting` cases that hit ssa_lower's catch-all
// panic ("nested function declaration `<name>` inside a block /
// switch") because P3.4 didn't reach module-top-level blocks.
//
// Implementation: extend `desugar_nested_fns` Pass 2 to walk every
// non-FnDecl top-level stmt (Block / If / While / For / Try /
// Switch / DoWhile) recursively, lifting nested FnDecls into the
// "__top" namespace with the same name-mangling shape as Pass 1's
// per-FnDecl walk. Renames apply to the entire top-level so call
// sites resolve to the lifted name.

// Direct top-level block.
{
  function inner1() { return 1; }
  console.log(inner1());           // 1
}

// Nested under top-level if.
if (true) {
  function inner2() { return 2; }
  console.log(inner2());           // 2
}

// Inside a top-level for-loop's body block.
for (let i = 0; i < 1; i = i + 1) {
  function inner3() { return 3; }
  console.log(inner3());           // 3
}

// Multiple nested fns at top-level under different shapes.
{
  function a() { return 'a'; }
  function b() { return 'b'; }
  console.log(a());                // a
  console.log(b());                // b
}

// Nested fn inside a top-level switch case body.
let kind = 'x';
switch (kind) {
  case 'x': {
    function sw() { return 'sw'; }
    console.log(sw());             // sw
    break;
  }
}

// Top-level FnDecl outside block still works.
function outer() { return 'outer'; }
console.log(outer());              // outer

// P3.4-followup-A2 — bare FnDecl as direct then/else of an if-stmt
// (no enclosing Block). `if (cond) function f() {}` parses with the
// then-branch as Stmt::FnDecl directly. Per ES annexB §B.3.4 the
// declaration parses but its binding is NOT hoisted to the outer
// scope (the bun reference also reports `f is not defined` if
// called from outside). tora's lift-to-top desugar makes the
// declaration parse + lower without panicking; user code can't
// reference the bare-form fn from outside the if (matches bun
// behavior — but the test262 cases that use this shape only check
// that parsing succeeds, never call into the body).
if (true) function bareThen() { /* not callable from outer */ }
console.log('bare-then-parsed');               // bare-then-parsed
if (false) function bareSkipped() {} else function bareElse() {}
console.log('bare-else-parsed');               // bare-else-parsed
