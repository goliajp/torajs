// S169 — `JSON.stringify(null)` per ES §25.5.2 → `"null"`.
// Prior to S169 the SSA-lower dispatcher panicked on Type::Ptr
// arms (the runtime SSA type both `null` and `undefined` lower to),
// so any `JSON.stringify(null)` call was a compile-time refusal.

// 1) Literal null.
console.log(JSON.stringify(null));

// 2) Ident-bound null.
const x = null;
console.log(JSON.stringify(x));

// 3) Const-bound null reused in two stringify calls — value
// stays consistent across SSA spilling.
const y = null;
console.log(JSON.stringify(y));
console.log(JSON.stringify(y));
