// RFC 20260710-optional-undefined-repr C3 — absent optional fields
// in a struct-annotated ObjectLit init desugar to explicit
// `field: undefined` (sentinel-capable slot types only: str + fn).
// Alias and inline-object annotations, partial literals, fn-body
// sites, and a mid-list required field all resolve; every filled
// field reads back as undefined.
type O = { tag?: string; cb?: () => string };
const o: O = {};
console.log(o.tag === undefined, o.cb === undefined);
console.log(o.tag, o.cb);
if (o.tag) {
  console.log("truthy");
} else {
  console.log("falsy");
}
const p: O = { tag: "hi" };
console.log(p.tag, p.cb === undefined, p.cb);
const inl: { s?: string } = {};
console.log(inl.s === undefined, inl.s);
function f(): number {
  const q: O = { cb: undefined };
  return q.tag === undefined ? 1 : 0;
}
console.log(f());
type M = { a?: string; b: string; c?: () => string };
const m: M = { b: "mid" };
console.log(m.a, m.b, m.c);
