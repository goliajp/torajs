// chunk 784 — the field's declared struct layout pins a direct
// ObjectLit rhs at member-assign sites (`h.sub = { v: 5 }` where
// Holder.sub declares an optional-field struct), mirroring the
// chunk-780 let-decl site.
type Inner = { v?: number };
type Holder = { sub: Inner };
type Other = { v: number };
function take(o: Other): number { return o.v }
console.log(take({ v: 9 }));
const seed: Inner = { v: 2 };
const h: Holder = { sub: seed };
h.sub = { v: 5 };
console.log(h.sub.v ?? 0);
