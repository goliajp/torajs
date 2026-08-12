// §13.5.5 / §6.1.6.2.1-2 — unary minus and ~ are LEGAL on a BigInt
// behind any, while unary plus (§7.1.4 ToNumber) throws; the Symbol
// rejects throw on every sign.
const b: any = 5n;
console.log(-b);
console.log(~b);
const n: any = -3n;
console.log(-n);
const z: any = 0n;
console.log(-z);
try {
  console.log("no-throw", +b);
} catch (e) {
  console.log("plus-threw");
}
const s: any = Symbol("x");
try {
  console.log("no-throw", -s);
} catch (e) {
  console.log("neg-sym-threw");
}
try {
  console.log("no-throw", ~s);
} catch (e) {
  console.log("bitnot-sym-threw");
}
console.log("after");
