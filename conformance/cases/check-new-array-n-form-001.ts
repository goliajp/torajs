// P0.10 — `new Array(n)` 1-arg numeric form per ES spec
// §23.1.2.1. Allocates an Array<Any> of length n with all slots
// set to ANY_NULL (tag=0, value=0). Pre-fix tora bailed at
// 'unknown identifier `__new_Array`' since the class-lowering
// desugar synthesizes `__new_C` factories for user classes only.
//
// The 0-arg and ≥2-arg forms are rewritten to array literals by
// `desugar_builtin_new` (commit bdba06c, 550/0/1). The 1-arg
// form needs runtime arr_alloc with explicit length-set, which
// the AST-level Call rewrite couldn't express (the intrinsic
// table expects a static SSA Type, but Array<Any> needs an
// arr_id intern'd at lower time). Handled directly in
// check.rs / ssa_lower.rs Expr::New arm instead.
//
// Implementation:
// * runtime_str.c — `__torajs_arr_alloc_any_filled(n)` helper.
//   Allocates header + n × 16-byte slots, sets cap = len = n,
//   memsets slots to 0 (tag=ANY_NULL, value=0). Bypasses the
//   pool (different stride from regular Array<T>).
// * ssa_lower.rs — declared the intrinsic in fn_table; intercept
//   `Expr::New { class_name = "Array", args.len() == 1 }` and
//   emit a Call with explicit Type::Arr(intern_arr_layout(Any))
//   so the .length / .at / etc. downstream paths typecheck.
// * check.rs — Expr::New for class_name = "Array" with 1 arg
//   returns `Array<Any>`. Arg must be Number-assignable.
//
// Out of scope at this commit: the slots' read returns ANY_NULL
// which `console.log`'s any_to_str renders as `"null"`, but JS
// spec says sparse-array reads return `undefined`. tora's Any
// tag system has no undefined slot — null serves as both. The
// fixture below tests only the .length path which matches bun
// exactly; downstream tests that assert `arr[0] === undefined`
// stay blocked until a proper undefined tag is added (separate
// substrate item).

let a = new Array(5)
console.log(a.length)                        // 5

let b = new Array(0)
console.log(b.length)                        // 0

let c = new Array(10)
console.log(c.length)                        // 10

let d = new Array(100)
console.log(d.length)                        // 100

// Iteration over a length-N array — body runs N times.
let n = new Array(3)
let count: number = 0
for (let i: number = 0; i < n.length; i = i + 1) { count = count + 1; }
console.log(count)                           // 3

// Combined with the existing 0-arg + ≥2-arg paths.
console.log(new Array().length)              // 0
console.log(new Array(1, 2, 3).length)       // 3
