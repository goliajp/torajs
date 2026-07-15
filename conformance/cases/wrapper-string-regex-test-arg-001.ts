// RFC 20260716-primitive-wrapper-substrate 刀 19 —
// `re.test(str)` / `re.exec(str)` ToString the string arg per
// ES §22.2.6.16 step 3 / §22.2.6.4 step 3. Closes the pass→incompat
// residual `test/built-ins/RegExp/prototype/test/S15.10.6.3_A1_T2.js`
// surfaced in the rotation-113 sweep (`__re.test(new String("123"))`
// — pre-fix checker rejected at "arg 0 must be string, got Any").
//
// Checker: `check_type_of_member_regex.rs` `test` / `exec` sigs
// relaxed `[String] -> Bool` / `[String] -> Nullable<Array<Str>>` to
// `[Any] -> Bool` / `[Any] -> Nullable<Array<Str>>`.
//
// SSA lower: `ssa_lower_call_regex_methods::lower_haystack` gains a
// non-Str/non-Substr arm that routes through `emit_to_string` (same
// coercion contract as the 刀 17-18 key lane). The `s_owned = true`
// path already existed for Substr → `substr_to_owned`; the coerced
// Str reuses it, so the caller's post-call drop is unchanged.

// Exact test262 S15.10.6.3_A1_T2 shape.
const __string: any = new String("123");
const __re = /((1)|(12))((3)|(23))/;
console.log(__re.test(__string));                                      // true
console.log(__re.test(__string) === (__re.exec(__string) !== null));   // true

// StringWrapper positive/negative haystack.
console.log(/abc/.test(new String("xyz-abc-def")));  // true
console.log(/abc/.test(new String("xyz-def")));      // false

// exec on wrapper — returns Array or null.
const m = /(a)(b)/.exec(new String("cab"));
console.log(m);                                       // [ "ab", "a", "b" ]
console.log(/foo/.exec(new String("bar")));           // null

// Number haystack → ToString(42) = "42".
console.log(/42/.test(42));                           // true
console.log(/^[0-9]+$/.test(1234));                   // true
console.log(/^[a-z]$/.test(42));                      // false

// Boolean haystack → ToString(true) = "true".
console.log(/true/.test(true));                       // true
console.log(/false/.test(false));                     // true

// Regression: primitive string haystack still works (fast borrow).
console.log(/abc/.test("has-abc-here"));              // true

// Regression: Substr haystack (a for-of-str binding).
let hits = 0;
for (const ch of "abcabc") {
  if (/[ac]/.test(ch)) hits++;
}
console.log(hits);                                    // 4
