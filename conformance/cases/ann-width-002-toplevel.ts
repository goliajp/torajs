// W1 (ann-width RFC) — top-level slot widths + `-0` literal seed.

// R4 — top-level f64 cell reassigned in a loop.
let h: number = 0.5;
let t: number = 0;
while (t < 10) {
  h = h / 2;
  t = t + 1;
}
console.log(h);

// R5 — `-0` literal flowing through a call into a division. The
// literal's fract() is 0, so the retired width heuristics narrowed it
// into a GPR path that aborted; the sign bit is f64 state (1/-0 must
// be -Infinity).
function signOf(z: number): number {
  return 1 / z;
}
const mz: number = -0;
console.log(signOf(mz));

// Top-level int slot reassigned from a named fn body — stays narrow
// only while every reaching value is integral; the f64 write from
// bumpFar poisons it module-wide (slot + named-fn store must agree).
let counter: number = 0;
function bumpFar(): number {
  counter = counter + 1.5;
  return counter;
}
console.log(bumpFar());
console.log(counter);
