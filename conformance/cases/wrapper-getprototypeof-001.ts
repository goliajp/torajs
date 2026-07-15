// RFC 20260716-primitive-wrapper-substrate 刀 15 — primitive-wrapper
// `Object.getPrototypeOf` answers the corresponding `<Ctor>.prototype`
// singleton per ES §10.1.1. Closes 1 of 3 pass→bug residual cases
// from handoff 112 (`15.2.3.2-2-22` — String wrapper prototype).
//
// Pre-fix `__torajs_anyv_get_proto_of_any`'s builtin-tag → proto_tag
// dispatch table had no arms for the wrapper cell tags (21/22/23);
// they fell to `_ => -1` → `VALUE_NULL_IMM`, and
// `Object.getPrototypeOf(new String()) === String.prototype`
// answered `false`.

// StringWrapper — the handoff residual case.
console.log(Object.getPrototypeOf(new String("abc")) === String.prototype);  // true
console.log(Object.getPrototypeOf(new String()) === String.prototype);       // true

// NumberWrapper mirror.
console.log(Object.getPrototypeOf(new Number(42)) === Number.prototype);     // true
console.log(Object.getPrototypeOf(new Number(3.14)) === Number.prototype);   // true

// BooleanWrapper mirror.
console.log(Object.getPrototypeOf(new Boolean(true)) === Boolean.prototype); // true
console.log(Object.getPrototypeOf(new Boolean(false)) === Boolean.prototype);// true

// Confirm the chain-parent stays Object.prototype (String.prototype's
// [[Prototype]] IS %Object.prototype% per §22.1.3.1).
console.log(Object.getPrototypeOf(String.prototype) === Object.prototype);   // true
