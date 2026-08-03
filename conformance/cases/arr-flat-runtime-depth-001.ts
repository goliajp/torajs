// Array.prototype.flat with a RUNTIME depth operand (§23.1.3.13
// step 2) — ToIntegerOrInfinity runs in the kernel: null → 0,
// numeric strings parse, NaN-coercing objects → 0, Infinity rides
// the full-depth flatten, and a Symbol operand throws through
// ToNumber before any element is read.
const a = [1, [2], [[3]]];
let depthNum: any = null;
console.log(a.flat(depthNum));
depthNum = "1";
console.log(a.flat(depthNum));
depthNum = 2;
console.log(a.flat(depthNum));
console.log(a.flat(Number.POSITIVE_INFINITY));
depthNum = {};
console.log(a.flat(depthNum));
depthNum = true;
console.log(a.flat(depthNum));
depthNum = -1;
console.log(a.flat(depthNum));
try {
  a.flat(Symbol());
} catch (e: any) {
  console.log("caught:", e.constructor.name);
}
