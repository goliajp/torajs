// ES §7.2.13 IsLooselyEqual step 6 — `String == Number` /
// `Number == String` coerces the string side via ToNumber (spec
// §7.1.4.1.1) + numeric compare. `''` → 0, `'1'` → 1, `'abc'`
// → NaN. NaN compares false against anything (spec §7.2.13
// step 1.b via fcmp's quiet-NaN semantic).
//
// Pre-fix tr type-rejected at check time with "strict equality
// requires same primitive type". This commit:
//   - check.rs: js_loose_eq_supported accepts (String, Number)
//     and (Number, String).
//   - ssa_lower.rs: new arm in the LooseEq/LooseNeq path that
//     emits `__torajs_str_to_number` + fcmp (Oeq), with `!=`
//     XOR'd to invert.
console.log("1" == 1);        // true
console.log("0" == 0);        // true
console.log("1" == 2);        // false
console.log("1.5" == 1.5);    // true
console.log("abc" == 0);      // false  (NaN == 0)
console.log("" == 0);         // true   (empty → 0)
console.log(1 == "1");        // true   (symmetric)
console.log(0 == "");         // true
console.log("1" != 1);        // false
console.log("abc" != 0);      // true

// Floating point round-trip
console.log("3.14" == 3.14);  // true
console.log("-1" == -1);      // true
console.log("1e3" == 1000);   // true

// Reverse + non-tie
console.log(2 != "1");        // true
