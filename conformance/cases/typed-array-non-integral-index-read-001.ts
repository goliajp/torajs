// `xs[1.5]` names the property "1.5", never element 1 (§7.1.19), and a
// typed array holds no such property: the read answers undefined the
// way an out-of-range index does. Every F64 index used to be truncated
// — xs[1.5] read xs[1], xs[-0.5] read xs[0], xs[NaN] read xs[0]. An
// integral f64 (2.0, xs.length / 2 when even) still reads the element.
// Elements here are f64 / string / any, the slot shapes that carry an
// undefined; an i64 / boolean slot's out-of-range read is the typed
// lane's recorded RangeError, and a non-integral index takes that same
// exit.
const xs: number[] = [7.5, 8.25, 9.75];
const i = 1.5, j = 2.0, k = -0.5;
console.log("read 1.5", xs[1.5], xs[i], "2.0", xs[2.0], xs[j], "-0.5", xs[k]);
const ss: string[] = ["a", "b"]; console.log("str[] 0.5", ss[0.5], ss[1.0]);
const half = xs.length / 2; console.log("len/2", half, xs[half], xs[Math.floor(half)]);
function at(a: number[], idx: number) { return a[idx]; }
console.log("fn idx", at(xs, 0.5), at(xs, 1), at(xs, 2.999));
let s = 0; for (let t = 0; t < xs.length; t += 0.5) { s = s + (xs[t] ?? 0); } console.log("half-step loop", s);
console.log("Infinity/NaN", xs[Infinity], xs[NaN], xs[-Infinity]);
const anys: any[] = [1, "x"]; console.log("any[] 0.5 / 1.0", anys[0.5], anys[1.0], anys[NaN]);
console.log("typeof", typeof xs[0.5], typeof xs[1.0], xs[0.5] === undefined, xs[0.5] == null);
console.log("in expr", (xs[0.5] ?? -1) + 1, xs[1.5] ? "truthy" : "falsy");
