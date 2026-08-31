// §20.1.2.14 step 1 — a primitive is frozen by definition, whatever
// spelling reaches the any lane (ShortStr immediate, heap Str,
// Symbol, BigInt, numbers, nullish); §19.1.2.6 step 1 — freeze
// returns a primitive unchanged.
const shortS: any = "ab";
const longS: any = "abcdefghij";
const runtimeS: any = ("abc" + "defgh" + Math.trunc(1)).slice(0, 8);
const big: any = 123n;
const sym: any = Symbol("x");
const num: any = 42;
const nil: any = null;
console.log(Object.isFrozen(shortS));
console.log(Object.isFrozen(longS));
console.log(Object.isFrozen(runtimeS));
console.log(Object.isFrozen(big));
console.log(Object.isFrozen(sym));
console.log(Object.isFrozen(num));
console.log(Object.isFrozen(nil));
console.log(Object.freeze(shortS) === shortS);
console.log(Object.freeze(big) === big);
console.log(typeof Object.freeze(sym));
// an actual object still freezes through the same entry
const o: any = { a: 1 };
Object.freeze(o);
console.log(Object.isFrozen(o));
try {
  o.a = 9;
} catch (e) {
  console.log("threw");
}
console.log(o.a);
