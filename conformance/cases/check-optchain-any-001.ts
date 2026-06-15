// Optional chain `obj?.field` on Type::Any receiver. Spec §13.3.9
// — the short-circuit value is `undefined`, not `null`. Pre-fix
// SSA-lower's `Expr::OptChain` arm only handled `Type::Obj(sid)`
// (static struct) and panicked on Any:
//   "ssa-lower: optional chain on non-struct obj type Any not yet
//   supported"
//
// Post-fix: Type::Any receiver routes through NaN-box tag-discriminated
// nullish check (mirrors the `??` Type::Any arm) — ANY_NULL=0 /
// ANY_UNDEF=5 short-circuits to ANY_UNDEF; otherwise dispatches to
// `lower_any_member_read` (the same Any.Member substrate that
// `obj.field` on Any uses), so dynobj entries / struct-cell fields /
// class-tag IC paths all work transparently.
//
// Primary motivation: `Object.getOwnPropertyDescriptor(o, k)?.value`
// — the canonical modern-JS pattern for optional-property probing,
// previously required `(d as any).value` workaround.

const arr = [10, 20, 30];
const d: any = Object.getOwnPropertyDescriptor(arr, "length");

console.log(d?.value);
console.log(d?.writable);
console.log(d?.enumerable);
console.log(d?.configurable);

// Short-circuit on undefined/null Any (the spec-correct miss path —
// previously this branch would have panicked at lowering time
// before the value-side tag check could discriminate).
const u: any = undefined;
console.log(u?.value);

const n: any = null;
console.log(n?.value);

// Dynobj-backed Any receiver — `obj?.k` on a dynamically-keyed Any
// goes through the same Any.Member substrate. Pre-fix this case
// also panicked at the SSA-lower OptChain arm.
const dyn: any = { greeting: "hi" };
console.log(dyn?.greeting);
console.log(dyn?.absent);
