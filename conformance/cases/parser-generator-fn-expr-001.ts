// P-PARSE.5 — generator function expressions per ES spec
// §15.5.3: `function*() {...}`, `function* named() {...}`,
// `let g = function*(args) {...}`. Pre-fix tora's parse_fn_expr
// bailed at the `*` immediately after `function` with
// 'expected `(`, got Star'. Test262's
// language/expressions/generators/* and
// language/expressions/arrow-function/dstr/ary-ptrn-* (which
// use sub-generator helpers) hit this pervasively.
//
// Implementation: parser detects `function*` at fn expression
// start, brace-balances the param list and body, and emits an
// empty placeholder Expr::ArrowFn (params: [], body: []).
// tora's existing generator substrate operates on
// function-declaration form (Stmt::FnDecl with is_generator
// true → desugar to a state-machine class); re-using it for
// expression form would require carrying is_generator through
// Expr::ArrowFn + the closure-lift path, which is multi-day
// substrate work that lands in P14 (generator full + tail
// call). For the parser milestone we accept the syntax so
// surrounding code compiles; cases that actually exercise
// the yield protocol from a function-expression generator
// remain blocked until P14.
//
// Test262 cases that just assert parse acceptance start
// passing now.

// Anonymous generator fn expr.
let g1 = function*() { yield 1 }
console.log(typeof g1)                       // function

// With typed params.
let g2 = function*(a: number, b: number) { yield a + b }
console.log(typeof g2)                       // function

// Named generator fn expr — the `named` is for stack traces /
// recursion only; doesn't bind into the surrounding scope.
let g3 = function* named() { yield "hello" }
console.log(typeof g3)                       // function

// In an array literal — the canonical test262 spread-obj
// helper shape.
let gens = [function*() {}, function*() {}]
console.log(gens.length)                     // 2

// Regression — non-generator function expression still works
// the existing way.
let f = function() { return 42 }
console.log(f())                             // 42

// Regression — function declaration form (the existing
// generator substrate) is unaffected.
function* dec(): Generator<number> {
  yield 10
  yield 20
}
let it = dec()
console.log(it.next().value)                 // 10
console.log(it.next().value)                 // 20
