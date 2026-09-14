// RFC 20260905 residue 1 — the three callback lanes the audit named
// as not reading FLAG_CLOSURE_RECV_FIRST: a promise handler, the
// scalar sort comparator (helper fast path and inline fallback), and
// reduce / reduceRight. Each now feeds a receiver-first callee the
// undefined box §23.1.3.24 / §23.1.3.30.2 / §27.2.5.4.1 prescribe. A
// named declaration reading `this` is the plain shell at every one
// of these sites today, so this file is the control (bun parity must
// hold before and after the one-shell merge); the flagged path is
// measured under TORAJS_DIAG_FORCE_RECV_FIRST (RFC §7c).
function F(this: any, v: number): number { console.log("then", typeof this, v); return v + 1; }
function K(this: any, e: any): number { console.log("catch", typeof this, e.message); return 3; }
function C(this: any, a: number, b: number): number { console.log("cmp", typeof this); return a - b; }
function S(this: any, a: string, b: string): number { console.log("scmp", typeof this); return a < b ? -1 : a > b ? 1 : 0; }
function R(this: any, acc: number, x: number): number { console.log("reduce", typeof this, acc, x); return acc + x; }
function R3(this: any, acc: number, x: number, i: number): number { console.log("reduce3", typeof this, i); return acc + x; }
const ints: number[] = [3, 1]; ints.sort(C); console.log(ints.join(","));
const fracs: number[] = [2.5, 0.5]; fracs.sort(C); console.log(fracs.join(","));
const strs: string[] = ["b", "a"]; strs.sort(S); console.log(strs.join(","));
console.log([1, 2, 3].reduce(R, 10));
console.log([1, 2].reduceRight(R, 0));
console.log([5, 6].reduce(R3, 0));
Promise.resolve(7).then(F).then((x) => console.log("after", x));
Promise.reject(new Error("boom")).catch(K).then((x) => console.log("after2", x));
