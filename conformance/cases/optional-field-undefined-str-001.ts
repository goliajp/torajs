// RFC 20260710-optional-undefined-repr C1 — an undefined LITERAL
// stored into a Nullable<Str> struct slot (ObjectLit init + member
// assign) lands the immortal sentinel cell, not NULL: strict-eq
// distinguishes undefined from null, print/JSON/struct-print render
// undefined, truthiness stays falsy, and explicit-null slots keep
// their exact behavior.
type O = { tag?: string };
const o: O = { tag: undefined };
console.log(o.tag === undefined, o.tag === null);
console.log(o.tag);
if (o.tag) {
  console.log("truthy");
} else {
  console.log("falsy");
}
console.log(o);
console.log(JSON.stringify(o));
const p: O = { tag: "hi" };
console.log(p.tag === undefined, p.tag === null, p.tag);
const q: O = { tag: "gone" };
q.tag = undefined;
console.log(q.tag === undefined, q.tag === null, q.tag);
const r: { s: string | null } = { s: null };
console.log(r.s === null, r.s === undefined, r.s);
const a: any = o.tag;
console.log(a === undefined, a);
const b: any = o;
console.log(b.tag === undefined, b.tag);
