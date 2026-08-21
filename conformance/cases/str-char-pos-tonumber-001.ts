// ES §22.1.3.{1,2,3,4,17} — `pos` / `count` is ToIntegerOrInfinity'd,
// not shape-checked. Every operand shape reaches the coercion.
console.log("abc".charAt("x"));
console.log("abcd".charAt("   +00200.0000E-0002   "));
console.log("abc".charAt("1"));
console.log("abc".charAt(true));
console.log("abc".charAt(null));
console.log("abc".charCodeAt("1"));
console.log("abc".codePointAt("2"));
console.log("abc".at("1"));
console.log("abc".at("-1"));
console.log("ab".repeat("2"));
console.log("abc".charAt(Number.NaN));

// A cell with a valueOf reaches the same step.
const box = { valueOf() { return "2"; } };
console.log("abcd".charAt(box as any));

// Side effects in the operand still fire exactly once.
let hits = 0;
const once = { valueOf() { hits++; return 1; } };
console.log("abc".charAt(once as any), hits);
