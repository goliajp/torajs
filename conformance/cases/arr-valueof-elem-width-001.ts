// `Object.prototype.valueOf` answers `this` (§20.1.3.7), so an array's
// valueOf hands back the very same array — not a copy of it.
//
// The width analysis had no arm for the name, so the call answered "no
// key" and whatever held the result never joined the receiver's class.
// A receiver widened by a fractional write then handed its f64 bits to
// a binding still compiled as integers, and the read reinterpreted
// them.

const xs: number[] = [1, 2, 3];
xs[0] = 1.5;
const v: number[] = xs.valueOf();
console.log(v[0], v[1], v[2]);

// widened after the call — one array, so one class either way
const ys: number[] = [4, 5];
const w: number[] = ys.valueOf();
ys[1] = 2.5;
console.log(w[0], w[1]);

// the element class can be widened by a method seed instead of a write
const zs: number[] = [6, 7];
zs.find((x: number): boolean => x > 6);
const u: number[] = zs.valueOf();
console.log(u[0], u[1]);

// it really is the same array, so a write through one is seen by the
// other
const ps: number[] = [8, 9];
ps[0] = 0.5;
const q: number[] = ps.valueOf();
q[1] = 3.5;
console.log(ps[0], ps[1], q[0], q[1]);

// an all-integral receiver stays narrow and unaffected
const ns: number[] = [10, 11];
const nv: number[] = ns.valueOf();
console.log(nv[0], nv[1]);

// the primitive wrappers answer their own value, untouched by this
const n: number = 3.25;
console.log(n.valueOf());
const s: string = "hi";
console.log(s.valueOf());
const b: boolean = true;
console.log(b.valueOf());
