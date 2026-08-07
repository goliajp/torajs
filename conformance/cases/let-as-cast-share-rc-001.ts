// rotation 325 — `const s = arr as any` (no annotation): an `as`
// cast is a value-layer pass-through, so the init is the same borrow
// a bare Ident init is — but the let lane's share table had no As
// arm, the binding took the borrow as if it owned it, and its
// scope-end drop stole the source binding's stake (census: zero
// incs, two decs on the array). The destructuring desugar mints
// exactly this let for every non-Ident source, which is how
// `const { 1: second } = arr as any` reached the census.
const arr = [1, 2, 3];
const s = arr as any;
console.log(s.length, arr.length);
const { 1: second, length: len } = arr as any;
console.log(second, len);
const o = { a: 7 };
const p = o as any;
console.log(p.a, o.a);
const { 5: missing = 99 } = [1] as any;
console.log(missing);
console.log("done");
