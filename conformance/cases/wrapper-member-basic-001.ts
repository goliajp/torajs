// RFC 20260716-primitive-wrapper-substrate 刀 3 — truthy + member
// ladder. Wrapper receivers are heap cells (always truthy per
// §7.1.2), and their .valueOf / .toString / .length / [i] read
// through to the wrapped primitive (view-through in
// __torajs_any_method_call / __torajs_any_length_get /
// __torajs_any_index_get).

// Truthy — heap identity always truthy regardless of wrapped value.
console.log(!!new Boolean(false));  // true
console.log(!!new Number(0));       // true
console.log(!!new Number(NaN));     // true
console.log(!!new String(""));      // true

// Number wrapper — valueOf reads [[NumberData]], toString formats it.
console.log(new Number(5).valueOf());   // 5
console.log(new Number(5).toString());  // "5"

// Boolean wrapper — valueOf reads [[BooleanData]], toString formats.
console.log(new Boolean(false).valueOf());  // false
console.log(new Boolean(true).toString());  // "true"

// String wrapper — valueOf/toString view-through to inner Str cell;
// length reads the wrapped string's code-unit count; [i] indexes into
// the wrapped string.
console.log(new String("hi").valueOf());    // "hi"
console.log(new String("hi").toString());   // "hi"
console.log(new String("hi").length);       // 2
console.log(new String("hi")[0]);           // "h"
console.log(new String("hi")[1]);           // "i"
