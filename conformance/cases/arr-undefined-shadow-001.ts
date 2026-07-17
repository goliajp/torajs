// Arr leg of the own-undefined-shadow family: an expando entry
// storing undefined shadows the builtin method surface (§10.1.8.1
// OrdinaryGet) — `arr.join = undefined; arr.join()` is the
// resolved-not-callable TypeError, and OrdinaryToPrimitive skips a
// shadowed toString instead of running the builtin join.

const a: any = [1, 2, 3];
a.join = undefined;
let threw = false;
try {
  a.join(",");
} catch (e) {
  threw = true;
}
console.log(threw); // true

// both toString and valueOf exhausted -> TypeError
const b: any = [4, 5];
b.toString = undefined;
let threw2 = false;
try {
  const s = "" + b;
} catch (e) {
  threw2 = true;
}
console.log(threw2); // true

// a patched valueOf rescues the coercion once toString is shadowed
const c: any = [6];
c.toString = undefined;
c.valueOf = function () {
  return "V";
};
console.log(String(c)); // V

// an untouched array keeps the builtin surface
const d: any = [7, 8];
console.log(d.join("-"), String(d)); // 7-8 7,8
console.log("done");
