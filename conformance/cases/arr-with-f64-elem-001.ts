// `xs.with(i, v)` has to convert its replacement to the array's element
// width before handing it to the helper.
//
// The helper's slot argument is spelled i64 because a slot is 8 bytes,
// not because the element is an integer. An integer replacement was
// passed through raw, so on an array whose elements are f64 the
// integer's own bits landed in an f64 slot and read back as 4.9e-322 —
// silently, with the process exiting 0. A fractional replacement took
// the other branch and raised "Array.with on f64 elements not yet
// supported (need IR bitcast)", which stopped being true once
// BitCastF64ToI64 existed.

const xs: number[] = [1, 2, 3];
xs[0] = 1.5;

// integer replacement into an f64-element array
const a: number[] = xs.with(1, 99);
console.log(a[0], a[1], a[2]);

// fractional replacement — the branch that used to refuse outright
const b: number[] = xs.with(2, 0.25);
console.log(b[0], b[1], b[2]);

// negative index still wraps per §23.1.3.39 step 3
const c: number[] = xs.with(-1, 7);
console.log(c[0], c[1], c[2]);

// the source is untouched
console.log(xs[0], xs[1], xs[2]);

// an all-integral array keeps the narrow path
const ints: number[] = [1, 2, 3];
const d: number[] = ints.with(0, 42);
console.log(d[0], d[1], d[2]);

// string elements are unaffected by the numeric branch
const strs: string[] = ["a", "b"];
const e: string[] = strs.with(0, "z");
console.log(e[0], e[1]);
