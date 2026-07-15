// RFC 20260716-primitive-wrapper-substrate 刀 13 — String receivers
// (both typed-Str primitive AND StringWrapper) get spec-correct
// `hasOwnProperty(key)` per ES §22.1.4 "String Exotic Object":
// `length` (non-configurable) plus every integer index
// `[0, [[StringData]].length)` are own properties; instance methods
// (`.toString`, `.valueOf`, ...) live on `String.prototype`, not
// own.
//
// Pre-fix:
// - typed-Str primitive `"abc".hasOwnProperty("length")` folded to
//   `ConstBool(false)` in `ssa_lower_call_universal_methods.rs`
//   ("primitives have no own properties in our subset"). Runtime
//   `__torajs_str_prop_has` shim + Str-tier emit — Substr
//   materializes through `substr_to_owned` before the runtime call.
// - StringWrapper `.hasOwnProperty(...)` fell out of the tag cascade
//   in `__torajs_any_prop_has` (no `Tag::StringWrapper` arm) and
//   answered 0 for every key. View-through the inner `[[StringData]]`
//   cell (刀 3 pattern) and delegate to the shared `str_index_has`.

// --- typed-Str primitive receiver (V3-18 m2.a auto-boxing) ---
console.log("abc".hasOwnProperty("length"));   // true
console.log("abc".hasOwnProperty("0"));        // true
console.log("abc".hasOwnProperty("1"));        // true
console.log("abc".hasOwnProperty("2"));        // true
console.log("abc".hasOwnProperty("3"));        // false (OOB)
console.log("abc".hasOwnProperty("-1"));       // false (non-canonical)
console.log("abc".hasOwnProperty("toString")); // false (proto, not own)

// Empty typed-Str — only `length` remains own.
console.log("".hasOwnProperty("length"));      // true
console.log("".hasOwnProperty("0"));           // false

// --- StringWrapper receiver ---
const s = new String("abc");
console.log(s.hasOwnProperty("length"));       // true
console.log(s.hasOwnProperty("0"));            // true
console.log(s.hasOwnProperty("1"));            // true
console.log(s.hasOwnProperty("2"));            // true
console.log(s.hasOwnProperty("3"));            // false
console.log(s.hasOwnProperty("toString"));     // false

// --- Non-Str wrappers stay non-indexed ---
console.log(new Number(42).hasOwnProperty("length"));    // false
console.log(new Number(42).hasOwnProperty("toString"));  // false
console.log(new Boolean(true).hasOwnProperty("length")); // false
