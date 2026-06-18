// ES §7.2.13 IsLooselyEqual steps 7-9 — `Boolean == String` /
// `String == Boolean` coerces both sides via ToNumber (Boolean →
// 0/1; String → spec ToNumber on the bytes), then numeric
// compare via fcmp Oeq. Same shape as the `String == Number`
// arm in `553c1c18`, with an extra ZExt+coerce on the Boolean
// side.
//
// Boolean == Number was already accepted by `js_loose_eq_supported`
// (it sits in the original Number/Boolean/Null arm); only the
// String pair was the gap.
console.log(true == 1);     // true
console.log(false == 0);    // true
console.log(true == 2);     // false
console.log(true == "1");   // true
console.log(false == "");   // true   ("" → 0; true → 1; mismatch? actually false → 0)
console.log(true == "x");   // false  (NaN == 1)
console.log(1 == true);     // true   (symmetric — already worked via num/bool arm)
console.log("1" == true);   // true
console.log(true != 0);     // true
console.log(false != "");   // false

// Float cases
console.log(true == "1.0"); // true
console.log(false == "0.0");// true

// Reverse non-tie
console.log(false != "1");  // true
