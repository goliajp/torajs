// RFC 20260716-primitive-wrapper-substrate 刀 20 — String receivers
// (both typed-Str primitive AND StringWrapper) get spec-correct
// `propertyIsEnumerable(key)` per ES §22.1.4 String Exotic Object
// + §22.1.5.1 `length` non-enumerability: canonical indices
// `[0, [[StringData]].length)` are enumerable → `true`; every other
// key (including `"length"`) → `false`.
//
// Pre-fix:
// - typed-Str primitive `"abc".propertyIsEnumerable(_)` folded to
//   `ConstBool(false)` in `ssa_lower_call_universal_methods.rs`
//   (刀 13 comment: "cap this fast-path to hasOwnProperty and let
//   propertyIsEnumerable keep folding to false"). Now shares the
//   same Str fast path via the new `str_prop_enumerable` intrinsic
//   which mirrors `str_prop_has` but skips `"length"`.
// - StringWrapper `.propertyIsEnumerable(...)` fell out of the tag
//   cascade in `__torajs_any_prop_enumerable` and answered 0 for
//   every key. Now has a `Tag::StringWrapper` arm parallel to
//   刀 13's `__torajs_any_prop_has` arm.

// --- typed-Str primitive receiver ---
console.log("abc".propertyIsEnumerable("0"));       // true
console.log("abc".propertyIsEnumerable("1"));       // true
console.log("abc".propertyIsEnumerable("2"));       // true
console.log("abc".propertyIsEnumerable("3"));       // false (OOB)
console.log("abc".propertyIsEnumerable("length"));  // false (§22.1.5.1)
console.log("abc".propertyIsEnumerable("-1"));      // false (non-canonical)
console.log("abc".propertyIsEnumerable("toString")); // false (proto)

// Empty typed-Str.
console.log("".propertyIsEnumerable("0"));          // false
console.log("".propertyIsEnumerable("length"));     // false

// --- StringWrapper receiver ---
const s = new String("abc");
console.log(s.propertyIsEnumerable("0"));           // true
console.log(s.propertyIsEnumerable("1"));           // true
console.log(s.propertyIsEnumerable("2"));           // true
console.log(s.propertyIsEnumerable("3"));           // false
console.log(s.propertyIsEnumerable("length"));      // false
console.log(s.propertyIsEnumerable("toString"));    // false

// Empty StringWrapper (NULL inner sentinel).
const e = new String();
console.log(e.propertyIsEnumerable("0"));           // false
console.log(e.propertyIsEnumerable("length"));      // false

// --- hasOwnProperty regression (刀 13 fast path shared with 刀 20
// via the same universal-methods arm — length still `true`).
console.log("abc".hasOwnProperty("length"));        // true
console.log(s.hasOwnProperty("length"));            // true
console.log("abc".hasOwnProperty("0"));             // true

// --- Non-Str wrappers stay non-enumerable-of-anything (only
// dynobj expandos + Arr indices would carry enumerable flags).
console.log(new Number(42).propertyIsEnumerable("length"));    // false
console.log(new Boolean(true).propertyIsEnumerable("length")); // false
