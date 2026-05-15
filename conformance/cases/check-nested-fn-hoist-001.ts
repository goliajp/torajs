// P3.4 — block-scoped / nested function declaration hoisting per
// ES spec Annex B §B.3.3 (FunctionDeclarations in IfStatement
// Blocks). Pre-P3.4 tora's check.rs / ssa_lower only handled
// FnDecl at the top of `ast.stmts`; nested FnDecls inside fn
// bodies (or inside if/while/for/try blocks within fn bodies)
// errored at 'unknown identifier' on every call site reference.
// Test262 5k sample shows ~170 cases blocked on this single
// pattern across annexB/language/* and built-ins/Array/* uses.
//
// Implementation: ast.rs new pass `desugar_nested_fns`. For each
// top-level FnDecl, walks its body recursively (including into
// if/block/while/for/try/switch nested stmt lists) and collects
// every nested FnDecl. Each collected fn:
//   1. Lifted to top-level ast.stmts with a mangled name
//      `__nested_<parent>_<name>_<uid>` to avoid collisions.
//   2. Original site is dropped from the enclosing block.
//   3. All `Expr::Ident("<name>")` references in the parent body
//      (including transitively into the nested fn's own body to
//      catch recursive self-refs) get rewritten to the mangled
//      name.
//
// Captures: this pass only handles the no-capture case. If the
// nested fn body references parent locals, ssa_lower will fail
// at the lifted FnDecl's body lower (lifted = top-level scope,
// can't see parent locals). Real closure-capturing nested fns
// need lift_arrow_fns-style env-passing, which is a P3.4-followup.

// Flat nested fn (no block).
function flatTest(): number {
  function inner() { console.log("inner") }
  inner()
  return 0
}
flatTest()                                    // inner

// Nested fn inside if-block.
function ifTest(): number {
  if (true) {
    function f() { console.log("if-f") }
    f()
  }
  return 0
}
ifTest()                                      // if-f

// Multiple nested fns in same parent.
function multiTest(): number {
  function a() { console.log("a") }
  function b() { console.log("b") }
  a()
  b()
  return 0
}
multiTest()                                   // a / b

// Nested fns in different blocks.
function blockTest(): number {
  if (true) {
    function p() { console.log("p1") }
    p()
  }
  if (true) {
    function q() { console.log("q1") }
    q()
  }
  return 0
}
blockTest()                                   // p1 / q1

// Recursive nested fn (self-reference in body).
function recurTest(): number {
  function fact(n: number): number {
    if (n <= 1) { return 1; }
    return n * fact(n - 1)
  }
  console.log(fact(5))
  return 0
}
recurTest()                                   // 120
