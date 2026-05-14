// P0.5 — function expressions / arrow expressions with
// unannotated params get param-type defaulted to Type::Any
// instead of TypeVar. Pre-fix the closure-lift desugar
// explicitly skipped genericization on `__closure_*` FnDecls
// because the M3 generic-call retargeter only fires on bare
// global FnDecl callees, leaving closure TypeVar signatures
// unresolvable. Result: every `var f = function(a, b) {...}`
// (or `let f = (a, b) => ...`) hit 'requires a type annotation'
// at typecheck — the test262 assert harness shape was DOA.
//
// Implementation:
// * ast.rs desugar_implicit_generics: when a `__closure_*`
//   FnDecl reaches the closure-skip arm, default each
//   unannotated param's type_ann to "any" BEFORE running the
//   return-ann sniff. With `x: any` in scope the sniff
//   correctly infers `return x → any`; without the order
//   trick the sniff bails to None and the lower defaults the
//   return type to Void.
// * ssa_lower indirect-call dispatch (Type::FnSig path): when
//   the target signature's param at index i is Type::Any AND
//   the actual arg's SSA type is concrete, box the arg via
//   the new box_to_any helper before passing. Symmetric for
//   the Closure-typed callee path (same wrapping at argv
//   build time).
//
// Effect: closure declarations with bare params accept any
// concrete arg per call. Body operations on the Any-typed
// params route through the P0.3 / P0.4 helpers (===, ToBoolean)
// already shipped, plus future P0.x sub-items as they land.

// var f = function(...) — assertEq pattern.
var assertEq = function(a, b) { return a === b }
console.log(assertEq(1, 1))                  // true
console.log(assertEq(1, 2))                  // false
console.log(assertEq("hi", "hi"))            // true
console.log(assertEq("hi", "ho"))            // false
console.log(assertEq(true, true))            // true

// let f = function(...) — same shape via let.
let cmp = function(a, b) { return a !== b }
console.log(cmp(5, 5))                       // false
console.log(cmp(5, 6))                       // true

// Arrow expression-body — return inference picks "any" via
// the body-walk sniff.
let pick = (a, b) => a === b
console.log(pick(true, true))                // true
console.log(pick(true, false))               // false
console.log(pick("a", "a"))                  // true

// Single-arg passthrough — works for any concrete payload.
var inc = function(x) { return x }
console.log(inc(5))                          // 5
console.log(inc("hi"))                       // hi

// Mixed call sites — each call boxes its own concrete arg
// independently. The closure body just compares Any-tagged
// values via the P0.3 path.
let same = (a, b) => a === b
console.log(same(1, 1))                      // true
console.log(same(true, true))                // true
console.log(same("x", "x"))                  // true
console.log(same(1, "1"))                    // false (diff tags)

// Regression — typed closures (explicit annotations) stay on
// the existing typed-tier path with no boxing overhead.
let typed = (a: number, b: number): number => a + b
console.log(typed(3, 4))                     // 7
