// S133-2 — `String(Any)` / `Number(Any)` callable coercion.
// Pre-S133 check rejected with "V3-18 m1.h.9 follow-up" since the
// `ok` predicate for Number/String didn't include Type::Any (only
// the concrete primitives Number/Boolean/Null/Undefined/String).
//
// ssa-lower already has the runtime helpers ready:
//   - `coerce_to_str(_, Type::Any)` is the same path console.log
//     uses for multi-arg dispatch (NaN-box tag → runtime ToString).
//   - `coerce_any_to_number(_, Type::F64)` calls
//     `__torajs_anyv_to_number` (strtod for heap-Str, NaN for
//     other heap kinds, identity for inline tags).
//
// Fix narrow: check.rs Number/String ok sets include Type::Any;
// ssa-lower Number/String arms dispatch Type::Any to the existing
// helpers.

// 1. Number-typed Any → ToString
let v1: any = 42;
console.log(String(v1));

// 2. Number-typed Any → ToNumber (identity through inline tag)
console.log(Number(v1));

// 3. String-typed Any → ToString (identity, rc-inc)
let v2: any = "3.14";
console.log(String(v2));

// 4. String-typed Any → ToNumber via strtod
console.log(Number(v2));

// 5. Boolean Any → ToString
let v3: any = true;
console.log(String(v3));

// 6. Null Any → ToString
let v4: any = null;
console.log(String(v4));

// 7. Undefined Any → ToString
let v5: any = undefined;
console.log(String(v5));

// 8. Undefined Any → ToNumber (NaN per §7.1.4)
console.log(Number(v5));

// 9. NaN string → Number → NaN
let v6: any = "not-a-number";
console.log(Number(v6));

// 10. Negative float string → Number
let v7: any = "-12.5";
console.log(Number(v7));
