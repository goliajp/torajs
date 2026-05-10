// V3-18 m1.h.13 — arrow fn default params at call sites.
// Before: `apply_default_args` only registered top-level FnDecl
// names, so `let f = (x = 5) => ...` lifted to `__closure_0` with
// the default still attached to the FnDecl, but the call site
// `f()` couldn't find it (looked up `f`, not `__closure_0`).
// Fix: walk LetDecl initialized to Ident("__closure_*") or
// Closure{fn_name: "__closure_*"} and register an alias so the
// user-visible name resolves to the synthetic closure name.
let f = (x: number = 5) => x + 1
console.log(f())
console.log(f(10))

let g = (a: number, b: number = 100) => a + b
console.log(g(1))
console.log(g(1, 2))

// Capturing arrow + default — alias map must walk the Closure
// shape, not just bare Ident.
let bias = 7
let h = (x: number = 3) => x + bias
console.log(h())
console.log(h(20))
