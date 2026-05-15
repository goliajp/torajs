// P1.1 + P1.2 + P1.5 + P1.7 + P1.8 — `undefined` is a distinct
// primitive value from `null` per ES spec §6.1.1 / §6.1.2.
// Pre-P1 tora aliased `undefined` to `Type::Null` end-to-end,
// silently wrong-ing four spec-mandated distinctions:
//
//   1. typeof undefined === "undefined"  (was "object")
//   2. ToString(undefined) === "undefined"  (was "null")
//   3. ToNumber(undefined) === NaN  (was 0)
//   4. undefined === null is false  (was true)
//
// This commit ships the substrate that fixes all four:
//
// * check.rs — Type::Undefined first-class enum variant.
//   `undefined` global ident now resolves to Type::Undefined
//   (was Type::Null). Nullable<T> assignability accepts both
//   Null and Undefined (P1.7's spec-correct shape).
//
// * runtime_str.c — ANY_UNDEF=5 tag distinct from ANY_NULL=0.
//   any_typeof: tag 5 → "undefined" (vs tag 0 → "object").
//   any_to_str: tag 5 → "undefined" (vs tag 0 → "null").
//   any_to_number_inner: tag 5 → NaN (vs tag 0 → 0).
//   any_to_bool: tag 5 → false (same as tag 0; spec aligned).
//   any_payload_eq: tag 5 reflexive (undefined === undefined);
//     the tag-equality short-circuit keeps undefined !== null.
//   any_add lt/rt switches: tag 5 → NaN coercion in numeric path.
//   print_any: tag 5 → "undefined".
//
// * ssa_lower.rs — Expr::TypeOf consults expr_types: if source
//   is Type::Undefined, emit "undefined" string literal directly
//   without lowering (mirrors the existing `undefined` global
//   ident shortcut). New `box_to_any_from_expr` helper picks
//   ANY_UNDEF=5 vs ANY_NULL=0 based on source frontend type.
//   Eq/Neq dispatch: lower_binop_with_ids passes operand
//   ExprIds so the Any-side packing reads expr_types and packs
//   the concrete side with tag 5 vs 0 accordingly.

// Typeof distinguishes null from undefined.
console.log(typeof undefined)               // undefined
console.log(typeof null)                    // object

let x = undefined
console.log(typeof x)                       // undefined
let y: any = undefined
console.log(typeof y)                       // undefined
let z: any = null
console.log(typeof z)                       // object

// Strict equality: null !== undefined.
console.log(undefined === null)             // false
console.log(undefined === undefined)        // true
console.log(null === null)                  // true

let a: any = undefined
let b: any = null
console.log(a === b)                        // false
console.log(a === undefined)                // true
console.log(b === null)                     // true
console.log(a === null)                     // false
console.log(b === undefined)                // false

// Loose-eq behavior unchanged at this commit (P1 spec-cleanup
// for == lands separately; current tora collapses the loose
// path through strict tag-equality, still gives `undefined ==
// null` as false today).
