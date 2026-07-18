// §23.1.2.1 step 4.b — new Array(len) with ToUint32(len) != len throws
// RangeError; valid integer lengths (incl. -0) allocate normally.
function probe(label: string, mk: () => unknown): void {
  try {
    mk();
    console.log(label, "no-throw");
  } catch (e) {
    console.log(label, e instanceof RangeError, e instanceof Error);
  }
}
probe("frac", () => new Array(4.5));
probe("nan", () => new Array(NaN));
probe("posinf", () => new Array(Number.POSITIVE_INFINITY));
probe("neginf", () => new Array(Number.NEGATIVE_INFINITY));
probe("maxval", () => new Array(Number.MAX_VALUE));
probe("minval", () => new Array(Number.MIN_VALUE));
probe("neg", () => new Array(-1));
probe("over32", () => new Array(4294967296));
console.log("ok3", new Array(3).length);
console.log("negzero", new Array(-0).length);
const m: number = 2.5;
probe("fracvar", () => new Array(m * 3));  // runtime f64 value lane
console.log("okvar", new Array(m + 1.5).length);
