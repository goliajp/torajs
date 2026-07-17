// §10.1.6.3 step 6.b — redefining a non-configurable non-writable
// data property with the SAME value is a no-op, judged by SameValue
// (§7.2.10), not pointer identity: two "abcd" string cells are the
// same value. Pre-fix the exact-bits approximation threw
// "Attempting to change value of a readonly property."

const obj: any = {};
Object.defineProperty(obj, "foo", { value: "abcd" });
Object.defineProperty(obj, "foo", { value: "abcd" }); // same value: ok
console.log(obj.foo); // abcd

// a DIFFERENT value still rejects
try {
  Object.defineProperty(obj, "foo", { value: "other" });
  console.log("no throw");
} catch (e) {
  console.log("caught:", e instanceof TypeError);
}

// numeric same-value across representations
Object.defineProperty(obj, "n", { value: 1 });
Object.defineProperty(obj, "n", { value: 1.0 }); // SameValue(1, 1.0)
console.log(obj.n); // 1

// SameValue(-0, +0) is FALSE — the redefine rejects even though
// they compare strict-equal (mixed int/double packing included)
const z: any = {};
Object.defineProperty(z, "zero", { value: -0 });
try {
  Object.defineProperty(z, "zero", { value: +0 });
  console.log("no throw");
} catch (e) {
  console.log("caught-zero:", e instanceof TypeError);
}
Object.defineProperty(z, "zero", { value: -0 }); // same -0: ok
console.log(1 / z.zero === -Infinity); // true (-0 preserved)

// NaN redefines with NaN (SameValue(NaN, NaN) is true)
Object.defineProperty(obj, "nan", { value: NaN });
Object.defineProperty(obj, "nan", { value: NaN });
console.log(Number.isNaN(obj.nan)); // true
console.log("done");
