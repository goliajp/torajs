// P2.1 — `var` declarations are hoisted to the top of the
// enclosing fn-body / top-level script per ES spec §14.3.2.1
// VariableStatement. Pre-init reads return undefined per spec
// §8.1 / §14.3.2; post-init reads return the assigned value.
// Pre-P2.1 tora aliased `var` to `Token::Let` so var was
// indistinguishable from let post-parse — block-scoped, no
// hoist, pre-declaration reads errored at "unknown ident".
//
// Implementation:
// * lexer.rs — `Token::Var` distinct from `Token::Let`.
// * parser.rs — accept `var name [: T] [= init];` same as
//   `let`, but thread `is_var: true` into the resulting
//   `Stmt::LetDecl`.
// * ast.rs — new field `is_var: bool` on Stmt::LetDecl.
// * ast.rs — new pass `desugar_var_hoist` walks every fn body
//   + top-level script: collects every is_var=true LetDecl,
//   emits a synthetic `let <name>: any = Uninit` at the body
//   top (initial value undefined per P1.3), replaces the
//   original site with an assignment if the source had an
//   init. var is fn-scoped not block-scoped — recurses into
//   if / for / while / try / switch / block but stops at
//   FnDecl boundaries.
// * Pipeline order: desugar_uninit_let → desugar_var_hoist.
//   uninit_let only sees user-written `let x;` (legal to
//   inline); var_hoist's synthetic Uninits aren't touched.
// * ssa_lower.rs — Any-typed slot assignment auto-boxes the
//   RHS via box_to_any (mirrors LetDecl init Any-box path).

// Pre-init read returns undefined.
console.log(typeof x)                         // undefined
console.log(x)                                // undefined
var x = 1
console.log(x)                                // 1
console.log(typeof x)                         // number

// var is fn-scoped, not block-scoped. The `if` block doesn't
// open a new scope for var.
function check(): number {
  if (true) {
    var inner = 42
  }
  console.log(inner)                          // 42 (visible outside block)
  return 0
}
check()

// Multiple var declarations of the same name = single hoisted decl.
var y;
console.log(typeof y)                         // undefined
var y = 10;
console.log(y)                                // 10
var y;
console.log(y)                                // 10 (re-decl is no-op)

// var in for-init still hoists to enclosing scope.
function loopCheck(): number {
  for (var i = 0; i < 3; i = i + 1) {
    // body
  }
  console.log(i)                              // 3 (visible outside loop)
  return 0
}
loopCheck()
