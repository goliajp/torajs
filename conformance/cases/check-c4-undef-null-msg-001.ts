// RFC C4 — `undefined as any` vs `null as any` ANY-box tag
// preservation. Spec §10.1.6 ToObject + §20.1.2.8 `Object.getOwnPropertyDescriptor`
// rely on ANY_UNDEF=5 vs ANY_NULL=0 to discriminate the throw site
// ("undefined is not an object" vs "null is not an object").
//
// Pre-fix `lower_as_cast` only routed primitives (i64/i32/f64/bool)
// through `box_to_any_from_expr`; Ptr-typed expressions (undefined
// and null both lower to ConstPtrNull at the value layer) passed
// straight through and got `box_to_any`'s default
// `ConstPtrNull → ANY_NULL=0` arm — silently collapsing
// `undefined as any` to `null as any`.
//
// User-visible vector: `console.log(undefined as any)` printed `null`
// pre-fix (NaN-box tag was ANY_NULL=0 so `__torajs_print_anyv`
// dispatched the null arm). Post-fix dispatches the undefined arm.
// Both inline-cast and let-bound forms exercise the same code path
// since SSA-lower hoists the cast into the LetDecl init.

console.log(undefined as any);
console.log(null as any);

const u: any = undefined as any;
console.log(u);

const n: any = null as any;
console.log(n);

// `typeof` over the boxed value also keys off the runtime tag —
// ANY_UNDEF=5 → "undefined", ANY_NULL=0 → "object" (per spec
// §13.5.3 `typeof null === "object"`).
console.log(typeof (undefined as any));
console.log(typeof (null as any));
