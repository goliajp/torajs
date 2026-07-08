// chunk 707 — object destructuring rest `{ p, ...rest }` (ES2018):
// rest binds a fresh object holding the source's remaining fields
// (desugars to the __spread_omit__ sentinel; typed as the source
// struct minus the destructured keys).
const o = { p: 1, q: 2, r: "s" };
const { p, ...rest } = o;
console.log(p, rest.q, rest.r);
console.log(rest);
const { q: qq, ...rest2 } = o;
console.log(qq, rest2.p, rest2.r);
const { ...all } = o;
console.log(all.p, all.q, all.r);
const { p: p3, q: q3, r: r3, ...none } = o;
console.log(p3, q3, r3, none);
function mk() { return { a: 10, b: 20 }; }
const { a, ...rb } = mk();
console.log(a, rb.b);
const { p: pd = 99, ...restd } = o;
console.log(pd, restd.q);
const inner = { z: 5 };
const src2 = { keep: inner, drop: 1 };
const { drop, ...restk } = src2;
console.log(drop, restk.keep.z, inner.z);
