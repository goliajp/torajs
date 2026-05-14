// P0.10 — `new Array(...)` MVP rewrite per ES spec §23.1.2.1
// Array constructor:
//   - 0 args  → empty array `[]` (length 0)
//   - ≥2 args → array literal `[a, b, c, ...]`
//
// Pre-fix tora bailed at typecheck with 'unknown identifier
// `__new_Array`' since the class-lowering desugar synthesizes
// `__new_C` factories for user classes only — built-in Array has
// no factory. Test262 uses `new Array()` and `new Array(a, b)`
// pervasively (~120+ cases blocked across the broader sample).
//
// The 1-arg-numeric form (`new Array(n)` → length-n array filled
// with undefined) needs runtime arr_alloc(n) with Any null-fill
// — substrate gap, deferred. The 1-arg-non-numeric form (`new
// Array("hello")` → `["hello"]`) is also deferred: distinguishing
// numeric vs non-numeric at compile time requires generic
// type-resolution that overlaps with the substrate work.
//
// Implementation: ast.rs `desugar_builtin_new` Pass 3 — for each
// `Expr::New { class_name = "Array", args }`, if 0 args or ≥2
// args, rewrite to `Expr::Array(args)` in place. The 1-arg shape
// leaves the `Expr::New` unchanged; typecheck still reports the
// missing factory so behavior is unchanged from before.

let a = new Array()
console.log(a.length)                        // 0

let b = new Array(1, 2, 3)
console.log(b.length)                        // 3
console.log(b[0])                            // 1
console.log(b[2])                            // 3

let c = new Array("x", "y", "z", "w")
console.log(c.length)                        // 4
console.log(c[0])                            // x

// Mixed-type array via new Array constructor.
let d: any[] = new Array(1, "two", true)
console.log(d.length)                        // 3
