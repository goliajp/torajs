// P3.2 — Type::Any property-bag substrate per ES spec §10.1
// (ordinary objects). `let x: any = {}` allocates a dynobj
// (hash-map backed, P3.1 substrate). Subsequent `x.foo = v` and
// `x.foo` reads route through dynobj_set / dynobj_get_tag /
// dynobj_get_value. Missing properties read as undefined per
// spec §10.1.5 (OrdinaryGet). Mixed-type values (number /
// string / bool / null) are tagged via the existing Any-box
// scheme (ANY_NULL=0 / ANY_BOOL=1 / ANY_I64=2 / ANY_F64=3 /
// ANY_HEAP=4 / ANY_UNDEF=5).
//
// Implementation:
// * P3.1 (commit c35aec4) — runtime_str.c ships
//   `__torajs_dynobj_*` family (alloc / get_tag / get_value /
//   set / has / delete / drop). Self-implemented Swift Dictionary-
//   style open-addressing + FNV-1a hash + linear probing.
// * P3.2 — ssa_lower wires:
//   * `let x: any = { ... }` → dynobj_alloc + per-field dynobj_set,
//     boxed as ANY_HEAP=4 (Any-box wrapping the dynobj ptr).
//   * `x.foo = v` (obj_ty == Any) → extract dynobj from Any-box
//     value field (offset 16), pack v as (tag, value), call
//     dynobj_set.
//   * `x.foo` (obj_ty == Any) → extract dynobj, call
//     dynobj_get_tag/value, box result as Any.
// * box_to_any.Type::Ptr arm: ConstPtrNull → ANY_NULL; other
//   Ptr → ANY_HEAP=4 with ptr value (was always ANY_NULL pre-P3.2,
//   which silently dropped dynobj ptrs).
// * check.rs Member access on Type::Any returns Type::Any (was a
//   hard reject); Member assign on Type::Any accepts any value.

let x: any = {}
x.a = 1
x.b = "hello"
x.c = true
x.d = null
console.log(x.a)                             // 1
console.log(x.b)                             // hello
console.log(x.c)                             // true
console.log(x.d)                             // null

// Missing property reads as undefined.
console.log(x.missing)                       // undefined
console.log(typeof x.missing)                // undefined
console.log(x.missing === undefined)         // true
console.log(x.missing === null)              // false

// Overwrite a property.
x.a = "now-string"
console.log(x.a)                             // now-string
x.a = 42
console.log(x.a)                             // 42

// Many properties — exercises the hash-map probe + grow path.
let y: any = {}
y.k1 = 1
y.k2 = 2
y.k3 = 3
y.k4 = 4
y.k5 = 5
y.k6 = 6
y.k7 = 7
y.k8 = 8
y.k9 = 9
y.k10 = 10
console.log(y.k1)                            // 1
console.log(y.k10)                           // 10
