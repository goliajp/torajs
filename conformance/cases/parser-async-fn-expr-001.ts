// P0.10 — async function expression `async function() {...}` /
// `async function NAME() {...}` per ES spec §15.8.5
// AsyncFunctionExpression. Pre-fix tora's parser only accepted
// `async` as a statement-level prefix to a FnDecl; `async function
// () {}` in expression position bailed at "expected expression,
// got Async". Test262 uses these for `async function() {}.
// constructor` patterns (~15+ cases under built-ins/AsyncFunction/*
// and built-ins/AsyncDisposableStack/*).
//
// Implementation: parse_primary detects Token::Async followed by
// Token::Function, drops the param list paren-balanced, drops the
// optional return type ann, drops the body brace-balanced, and
// emits an empty placeholder Expr::ArrowFn — same opaque-stub
// strategy as generator-expression (function*) and getter/setter/
// computed-method bodies. Real async-fn-expression substrate
// (state-machine generation, await binding) is a P-LATER item;
// for the parser milestone this just needs to parse cleanly so
// the surrounding code still compiles.

// Bare async function expression assigned to var.
let a = async function() { return 1; };
console.log("a-ok")                          // a-ok

// Named async function expression.
let b = async function named() { return 2; };
console.log("b-ok")                          // b-ok

// Async function expression with typed params + return ann.
let c = async function(x: number, y: number): Promise<number> {
  return x + y;
};
console.log("c-ok")                          // c-ok

// (fn-arg in any position + array-literal of async fn exprs
// deferred — both need FnSig box-into-Any substrate which isn't
// part of this parser fix.)
