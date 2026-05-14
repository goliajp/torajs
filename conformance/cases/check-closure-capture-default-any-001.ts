// P0.10 — capturing-closure user params default to Type::Any when
// un-annotated, matching the existing non-capturing closure
// default-Any path in `desugar_implicit_generics`. Pre-fix the
// `__env`-prefixed branch ran return-type sniffing then `continue`d
// without applying the default-Any treatment. Result: capturing
// arrows / function exprs like
//
//   xs.forEach(function (value) { sideEffect(); })
//
// where `value` is unannotated AND the receiver is a heterogeneous
// (Array<Any>) literal so `infer_anonymous_closure_params` can't
// resolve the elem type, hit 'parameter `value` of function
// `__closure_N` requires a type annotation' at typecheck. Test262's
// annexB/built-ins/RegExp/legacy-accessors/* (~50 cases) and many
// other capturing-callback patterns are blocked on this single
// shape.
//
// Implementation: ast.rs `desugar_implicit_generics` — when the
// FnDecl is `__closure_<N>` AND first param is `__env` (capturing),
// also iterate the rest of the params and default un-annotated
// slots to `"any"` before the existing return-type sniff. Same
// "any" default as the non-capturing closure branch already emits;
// the two paths converge.

function main() {
  // Capturing path: outer `counter` forces `__env` synthesis on
  // the closure's lifted FnDecl. Param `value` is unannotated.
  let counter = 0
  let arr: any[] = [undefined, null, true]
  arr.forEach(function (value) {
    counter = counter + 1
  })
  console.log(counter)                       // 3
}
main()

// Capturing function-expression with unannotated param. Pre-fix
// `a` would fail at typecheck.
function fnExprTest() {
  let result = 0
  let xs: any[] = [10, 20, 30]
  xs.forEach(function (a) {
    result = result + 1
  })
  console.log(result)                        // 3
}
fnExprTest()

// Capturing arrow + unannotated param (regression — arrow also
// goes through the same desugar path).
function arrowTest() {
  let counter = 0
  let xs: any[] = [1, 2, 3]
  xs.forEach((s) => { counter = counter + 1 })
  console.log(counter)                       // 3
}
arrowTest()
