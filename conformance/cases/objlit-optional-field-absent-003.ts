// Only an optional member (`a?: T`) may be left out of a literal, on
// every spelling of the type; a spread copies an optional member only
// when the source owns it, so an earlier value of another type stays.

type O = { s?: string; b: number };
const p: O = { b: 2 };
const q: O = { s: "x", b: 3 };
const m1 = { s: 5, ...p };
console.log(m1.s, "s" in m1, typeof m1.s);
const m2 = { s: 5, ...q };
console.log(m2.s, "s" in m2, typeof m2.s);
const m3 = { s: "pre", ...p, c: true };
console.log(m3.s, Object.keys(m3).join(","));
const m4 = { s: null, ...p };
console.log(m4.s, "s" in m4);

// before any write the absent field reads as undefined
type P = { a?: number; b: number };
const r: P = { b: 1 };
console.log(r.a + 1, r.a === undefined, typeof r.a);
const nan: P = { a: NaN, b: 1 };
console.log(nan.a === undefined, "a" in nan, Number.isNaN(nan.a));
const und: P = { a: undefined, b: 1 };
console.log(und.a === undefined, "a" in und, Object.keys(und).join(","));

// a repeated key is one property
const dup: P = { b: 1, b: 2 };
console.log(dup.b, "a" in dup, Object.keys(dup).join(","), JSON.stringify(dup));

interface I {
  a?: number;
  b: number;
}
interface J extends I {
  c?: string;
}
const j: J = { b: 2 };
console.log(j.c, "c" in j, "a" in j, Object.keys(j).length);

type G<T> = { v?: T; b: number };
const g: G<string> = { b: 3 };
console.log(g.v, "v" in g, Object.hasOwn(g, "b"));
function gen<T>(x: { v?: T; b: number }): number {
  return x.b + ("v" in x ? 100 : 0);
}
console.log(gen<number>({ b: 4 }));

type N = { inner: { s?: string; n: number }; tag?: string };
const n: N = { inner: { n: 5 } };
console.log(n.inner.s, "s" in n.inner, "tag" in n, Object.keys(n).join(","));

// an optional member that admits null as well
type Z = { z?: number | null; k: number };
const z: Z = { k: 1 };
const zn: Z = { z: null, k: 1 };
console.log(z.z, "z" in z, zn.z, "z" in zn);
