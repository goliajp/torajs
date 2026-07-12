// RegExp v flag (unicodeSets) — chunk B1: cp set algebra.
// ClassSetExpression folds eagerly into a CpRangeSet (union /
// `&&` intersection / `--` subtraction / nested classes / eager
// complement) and lands on CharClass as byte bitmap + owned_ranges.

// union of nested class and character
console.log(/^[[0-9]_]+$/v.test("09_"), /^[[0-9]_]+$/v.test("a"));
// subtraction, chained
console.log(/^[[0-9]--[4-6]]+$/v.test("0139"), /^[[0-9]--[4-6]]+$/v.test("5"));
console.log(/^[[0-9]--[0-3]--[8-9]]$/v.test("5"), /^[[0-9]--[0-3]--[8-9]]$/v.test("9"));
// intersection with class escape and property operands
console.log(/^[[0-9]&&[0-7]]+$/v.test("07"), /^[[0-9]&&[0-7]]+$/v.test("8"));
console.log(/^[\d&&[0-7]]$/v.test("7"), /^[d&&[0-9]]$/v.test("d"));
console.log(
  /^[\p{ASCII_Hex_Digit}&&\p{Nd}]+$/v.test("059"),
  /^[\p{ASCII_Hex_Digit}&&\p{Nd}]+$/v.test("af")
);
// intersection with negated nested class
console.log(/^[[a-z]&&[^aeiou]]+$/v.test("bcd"), /^[[a-z]&&[^aeiou]]+$/v.test("e"));
// top-level negation is an eager complement over the cp domain
console.log(/^[^[0-9]]$/v.test("a"), /^[^[0-9]]$/v.test("5"), /^[^[0-9]]$/v.test("π"));
// v implies unicode cp semantics
console.log(/^.$/v.test("😀"), /^[\u{1F600}-\u{1F64F}]$/v.test("😀"));
console.log(/^\p{L}+$/v.test("abcα"), /^\P{L}$/v.test("5"));
// \P{} as a set operand
console.log(/^[\P{ASCII}]$/v.test("ü"), /^[\P{ASCII}]$/v.test("a"));
// flags surface
const re = /[a]/v;
console.log(re.unicodeSets, re.flags, /x/.unicodeSets);
// dynamic construction
console.log(new RegExp("[[0-9]--[0-4]]", "v").test("7"));
// escaped punctuators + literal non-ASCII cp
console.log(/^[\-\&]$/v.test("-"), /^[π]$/v.test("π"));
// global scan
const m = "a1b2".match(/[[0-9]]/gv);
console.log(m ? m.length : 0);
// console print carries the v flag
console.log(/[[a-b]&&[b-c]]/v);
