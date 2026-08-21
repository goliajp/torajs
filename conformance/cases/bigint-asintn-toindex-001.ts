// ES §21.2.2.{1,2} step 1 is ToIndex(bits) per §7.1.22: coerce, then
// range-test the Number itself — NaN folds to 0, ±∞ and anything past
// 2**53-1 are RangeErrors.
console.log(BigInt.asIntN(8, 255n), BigInt.asUintN(8, 255n));
console.log(BigInt.asIntN(3.9, 255n));
console.log(BigInt.asIntN(NaN as any, 255n));
console.log(BigInt.asIntN("8" as any, 255n));
console.log(BigInt.asUintN("4" as any, 255n));
console.log(BigInt.asIntN(true as any, 255n));
console.log(BigInt.asIntN(0, 255n));
for (const bad of [-1, -2.5, "-2.5", -Infinity, 9007199254740992, Infinity]) {
  try {
    BigInt.asIntN(bad as any, 0n);
    console.log("NO THROW", bad);
  } catch (e) {
    console.log("RangeError", e instanceof RangeError);
  }
}
// Trailing args are ignored but still evaluate.
let hits = 0;
const once = { valueOf() { hits++; return 8; } };
console.log(BigInt.asIntN(once as any, 255n), hits);
console.log(BigInt.asIntN(8, 255n, 1 as any));
