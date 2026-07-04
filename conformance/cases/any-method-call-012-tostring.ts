// any-method-call RFC C4+ — generic .toString() on str / bool
// receivers: heap Str and Substr cells are identity (+1 on the same
// cell), ShortStr immediates return their own bits, booleans mint
// "true"/"false".
const s: any = "hello world this is a long heap string";
console.log(s.toString());
console.log(s); // receiver survives the identity +1 (ledger balance)
const ss: any = "hi";
console.log(ss.toString());
const b: any = true;
console.log(b.toString());
const b2: any = false;
console.log(b2.toString());
// chained onto the identity result
console.log(s.toString().length);
console.log(ss.toString().toUpperCase());
// Substr-view receiver stays identity too
console.log(s.slice(0, 11).toString());
