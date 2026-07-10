// chunk 785 — nested object literals pin through the declared
// field's struct layout: `{ sub: { v: 1 } }` lowers the inner
// literal against Inner's slot reprs. previously the inner literal
// first-matched the same-shaped layout of an unrelated TypeDecl
// (Other below) and the whole program's output was silently
// swallowed.
type Inner = { v?: number };
type Holder = { sub: Inner };
type Other = { v: number };
function take(o: Other): number { return o.v }
const h: Holder = { sub: { v: 1 } };
console.log(h.sub.v ?? 0);
console.log(take({ v: 9 }));
h.sub = { v: 5 };
console.log(h.sub.v ?? 0);
console.log("end");
