// T-19.p (v0.5.0) — capturing arrow expr-body return-type
// inference. Pre-T-19.p, `let x = 10; let cb = (v: number) =>
// v + x` failed: the static sniff in desugar_implicit_generics
// only saw the closure body's params + body let-decls, not the
// captured outer ident `x`. cb's FnDecl ended with `: void`
// and the call site rejected its result.
//
// New: pre-pass walks ast.stmts to collect annotations for all
// top-level let-decls AND all FnDecl params (including parent
// fn params for nested closures). The capturing-closure path
// (FnDecl with __env first param) seeds infer_return_ann with
// these outer binds before walking the body. Tora's SSA-level
// de-shadow means the bind table just needs ANY matching
// annotation — lexical correctness isn't required.

let factor = 10
let cb = (v: number) => v * factor
console.log(cb(3))                // 30

// Multi-capture
let a = 100
let b = 200
let multi = (x: number) => x + a + b
console.log(multi(5))             // 305

// Capturing closure inside fn body — captured `x` is the
// enclosing fn's param, lifted to a top-level FnDecl by
// lift_arrow_fns. The pre-scan catches FnDecl params too so
// the closure body's `x + y` resolves.
function makeAdder(x: number): (y: number) => number {
  return (y: number) => x + y
}
let add5 = makeAdder(5)
console.log(add5(7))              // 12

// Real workload: Promise.then with a capturing-arrow cb. The
// runtime (T-15.g.5) already supported closure-cb dispatch;
// only the type-system inference was missing.
let scale = 100
let p = Promise.resolve(7).then((v: number) => v * scale)
console.log(await p)              // 700
