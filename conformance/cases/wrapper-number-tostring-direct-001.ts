// RFC 20260716-primitive-wrapper-substrate 刀 10 — NumberWrapper
// ToString direct-read fast path in `any_to_str` (coerce.rs).
//
// Pre-fix `any_to_str(NumberWrapper)` fell through to the general
// `heap_to_primitive` dispatch → method table lookup → valueOf/
// toString call → primitive ToString. All that round-trip just to
// read the `[[NumberData]]` f64 at offset 8 and hand it to the
// existing `__torajs_f64_to_str` intrinsic. Mirrors the direct-read
// shortcuts on StringWrapper (刀 2b) and BooleanWrapper (刀 2c).
//
// Impact: every `String(new Number(x))` / `new Number(x) + ""` /
// `"prefix" + new Number(x)` site skips ~3 heap indirections plus
// the throw-check paperwork.

console.log(String(new Number(42)));         // "42"
console.log(String(new Number(3.14)));       // "3.14"
console.log(String(new Number(-0)));         // "0"
console.log(String(new Number(NaN)));        // "NaN"
console.log(String(new Number(Infinity)));   // "Infinity"
console.log(String(new Number(-Infinity)));  // "-Infinity"

// Concat lane — any_add via StringWrapper coercion picks up the
// direct-read too (Str-side reads any_to_str on the number arm).
console.log(new Number(7) + "");             // "7"
console.log("v=" + new Number(9));           // "v=9"

// Template literal lane — same any_to_str call site.
console.log(`n=${new Number(1.5)}`);         // "n=1.5"
