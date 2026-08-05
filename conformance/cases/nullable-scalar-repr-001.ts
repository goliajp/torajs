// A scalar `T | null` had nowhere to put the null: the slot held the
// in-band 0 sentinel a pointer-shaped T can spare, which for a number
// is a legitimate `0` and for a boolean a legitimate `false`. So the
// null did not merely print wrong — it was absent, and every test
// written to guard against it answered the value's answer.
const n: number | null = null;
console.log("num let:", n);
console.log("num eq:", n === null, n == null);
console.log("num typeof:", typeof n);

const b: boolean | null = null;
console.log("bool let:", b);
console.log("bool eq:", b === null);

// The guard has to actually guard: an `if (x !== null)` on a null must
// not run its narrowed branch.
if (n !== null) {
  console.log("num narrowed WRONG:", n + 1);
} else {
  console.log("num else:", n);
}

// Param and return positions carry it too.
function viaParam(x: number | null): string {
  return x === null ? "null-param" : "value " + x;
}
console.log("param null:", viaParam(null));
console.log("param value:", viaParam(4));

function viaReturn(): number | null {
  return null;
}
console.log("return:", viaReturn());

// A narrowed value still has to be usable as the scalar it is.
let m: number | null = 5;
if (m !== null) {
  console.log("narrowed arith:", m * 2);
}
m = null;
console.log("after reassign:", m);

// An inferred array literal anchored on a nullable scalar element:
// the element materializes boxed, so the literal has to take the
// tagged lane rather than store box bits into an 8-byte slot.
const arr = [n, 1];
console.log("inferred arr:", arr[0], arr[1]);
const nonNull: number | null = 3;
console.log("inferred arr value:", [nonNull, 1][0]);
// The annotated spelling agrees.
const ann: (number | null)[] = [n, 1];
console.log("annotated arr:", ann[0], ann[1]);

// A pointer-shaped `T | null` keeps the in-band sentinel — it has a
// spare bit pattern — and was already correct.
const s: string | null = null;
console.log("str let:", s, s === null);
console.log("str arr:", [s, "x"][0]);

// A struct field was the one position the box tax already covered.
type Holder = { v: number | null };
const h: Holder = { v: null };
console.log("field:", h.v, h.v === null);
