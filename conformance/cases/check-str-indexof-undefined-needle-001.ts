// S235 — `String.{indexOf,lastIndexOf,includes,startsWith,endsWith,
// search}(undefined)` 1-arg-undef-needle per ES §22.1.3.{8,10,5,21,7,16}
// step 1-3: ToString(undefined) = "undefined". The needle slot accepts
// the interned literal at lower-time; the helper's (Str, Str) ABI sees
// a concrete pointer instead of ConstPtrNull.
console.log("hello".indexOf(undefined));
console.log("undefined".indexOf(undefined));
console.log("hello".lastIndexOf(undefined));
console.log("undefined".lastIndexOf(undefined));
console.log("hello".includes(undefined));
console.log("undefined".includes(undefined));
console.log("hello".startsWith(undefined));
console.log("undefined".startsWith(undefined));
console.log("hello".endsWith(undefined));
console.log("undefined".endsWith(undefined));
console.log("hello".search(undefined));
console.log("undefined".search(undefined));
