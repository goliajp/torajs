// RFC 20260716-primitive-wrapper-substrate 刀 6 — emit_to_number /
// emit_to_string Undef arm. ES §7.1.4 ToNumber(undef) = NaN /
// §7.1.17 ToString(undef) = "undefined"; pre-fix the SSA layer
// collapsed Type::Undefined to Ptr+ConstPtrNull and routed both
// through the null arm (ToNumber=0 / ToString="null"). The checker
// still has the frontend Type::Undefined, so the arm picks by it.

// `new Number(undefined)` runs its ToNumber path — inner NaN.
console.log(new Number(undefined).valueOf());          // NaN
console.log(String(new Number(undefined)));            // "NaN"
console.log(new Number(void 0).valueOf());             // NaN

// `new String(undefined)` runs its ToString path — inner "undefined".
console.log(new String(undefined).valueOf());          // "undefined"
console.log(new String(undefined).length);             // 9
console.log(new String(void 0).valueOf());             // "undefined"

// `new Boolean(undefined)` runs its ToBoolean path — inner false.
console.log(new Boolean(undefined).valueOf());         // false
console.log(new Boolean(void 0).valueOf());            // false

// Bare-callable coercion mirrors — chains through emit_to_number /
// emit_to_string with the same Undef check.
console.log(Number(undefined));                        // NaN
console.log(String(undefined));                        // "undefined"
console.log(Boolean(undefined));                       // false
