// `Array.from(src)` and the binding that holds its result must stay in
// one width class.
//
// `Array` is a global namespace ident, so the receiver lookup that keys
// a method call's result failed and the call answered "no key at all".
// With no key nothing joined the product to its binding: the binding
// widened on a fractional write while the copied source stayed narrow,
// and reading it back gave 1e-323 — silently, exit 0.
//
// The product memcpys the source's slots (the one-argument path is an
// arr_slice clone), so the two share one repr class, the same way
// `slice` and `concat` already did.

const u: number[] = Array.from([1, 2, 3]);
u[0] = 1.5;
console.log(u[0], u[1], u[2]);

// widened from the source side instead of the product side
const src: number[] = [4, 5, 6];
src[1] = 0.5;
const v: number[] = Array.from(src);
console.log(v[0], v[1], v[2]);

// an all-integral class stays narrow
const w: number[] = Array.from([7, 8]);
console.log(w[0], w[1]);

// the source is independent of the copy
const s2: number[] = [1, 2];
const c2: number[] = Array.from(s2);
c2[0] = 9.5;
console.log(s2[0], s2[1], c2[0], c2[1]);

// string elements are unaffected
const strs: string[] = Array.from(["a", "b"]);
console.log(strs[0], strs[1]);

// the mapFn form still works
const m: number[] = Array.from([1, 2], (x: number): number => x * 2);
console.log(m[0], m[1]);
