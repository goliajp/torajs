// An `any[]` element read at an index typed number (f64) is a [[Get]]:
// an accessor element runs its getter each time (and a throw from it
// reaches the catch), and a hole continues to the prototype. The
// integer arm of the f64-index read used to take the plain slot read,
// which answered undefined for both.
const xs: any[] = [1, 2, 3];
let calls = 0;
Object.defineProperty(xs, 1, {
  get() {
    calls = calls + 1;
    return 40 + calls;
  },
  configurable: true,
});
let h = 0.5;
for (let r = 0; r < 3; r++) {
  const k = h * 2;
  console.log(xs[k], calls);
  h = h + 0.5;
}
let f = 0.25;
f = f * 4;
console.log(xs[f], xs[f], calls);
// the same element through an integer index
for (let i = 0; i < xs.length; i++) console.log("i", xs[i]);
console.log(calls);

// a throwing getter
const ts: any[] = [0, 0];
Object.defineProperty(ts, 0, {
  get() {
    throw new Error("boom");
  },
});
let g = 0.5;
g = g * 0;
try {
  console.log(ts[g]);
} catch (e) {
  console.log("caught", (e as Error).message);
}

// a hole reads through to the prototype
const ys: any[] = [1, , 3];
(Array.prototype as any)[1] = "proto";
let q = 0.5;
q = q * 2;
console.log(ys[q]);
for (let i = 0; i < ys.length; i++) console.log("y", ys[i]);
delete (Array.prototype as any)[1];
console.log(ys[q]);
