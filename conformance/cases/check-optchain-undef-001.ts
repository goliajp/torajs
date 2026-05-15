// P3.5 — `obj?.field` returns Type::Any. Miss path emits ANY_UNDEF=5
// (per ES spec §13.3.9 — short-circuit value is undefined). Hit path
// loads the field, boxes as Any. `??` on Any-lhs reads the box's tag
// at offset 8; ANY_NULL=0 / ANY_UNDEF=5 → use rhs, otherwise unbox
// lhs to rhs's SSA type and use it.
//
// Pre-P3.5 result was field-typed with ConstPtrNull/0 sentinel on
// miss — silently wrong: `typeof obj?.x` returned the field's typed-
// tier label even on miss; print emitted "null" instead of "undefined";
// `obj?.x === undefined` worked only by accident (ConstPtrNull == 0
// happened to bit-compare equal to ANY_UNDEF in the test case).
//
// Spec-correct now requires:
//   - typeof: "number" on hit, "undefined" on miss
//   - strict eq with undefined: false on hit, true on miss
//   - nullish ??: lhs unboxed on hit, rhs on miss

type Pt = { x: number };
let q: Pt | null = { x: 7 };
let p: Pt | null = null;

// typeof — hit path returns the field's type label, miss returns
// "undefined". The field's value is Any-boxed but typeof unboxes
// the tag and dispatches to the JS spec label.
console.log(typeof q?.x);            // "number"
console.log(typeof p?.x);            // "undefined"

// Strict equality with undefined — miss must equal undefined per
// spec. ANY_UNDEF and the literal `undefined` both have tag=5.
console.log(q?.x === 7);             // true
console.log(p?.x === undefined);     // true
console.log(q?.x === undefined);     // false

// Nullish coalescing — miss takes rhs, hit unboxes lhs and uses it.
// rhs's SSA type drives the unbox dispatch (I64 / F64 / Bool /
// refcounted heap ptr).
console.log(q?.x ?? -1);             // 7
console.log(p?.x ?? -1);             // -1
console.log(p?.x ?? "fallback");     // "fallback"
