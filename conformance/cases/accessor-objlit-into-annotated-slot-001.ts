// An object literal with accessor slots meeting a slot that was
// ANNOTATED. The literal's layout spells the accessor `__getter_v`, a
// closure; the annotation spells `v`, a number, and both the checker
// and the three lowering lanes compare layouts field by field — so
// every one of these was refused, against a `tsc --strict` that
// accepts all of them.
//
// The rule is tsc's: an accessor slot projects onto the property it
// implements and the ordinary comparison runs against the projection.
// The annotation only gates admission — the binding keeps the
// LITERAL's layout, because a binding retyped to `{ v: number }`
// stops taking the accessor read lane and reads the closure pointer
// as a number.
let store = 1;

// getter only
const a: { v: number } = { get v() { return 7; } };
console.log("a", a.v);

// getter + setter pair
const b: { v: number } = { get v() { return store; }, set v(x) { store = x; } };
b.v = 9;
console.log("b", b.v, store);

// through a type alias
type Box = { v: number };
const c: Box = { get v() { return 7; } };
console.log("c", c.v);

// mixed with an ordinary data field, declaration order preserved
const d: { a: number; v: number } = { a: 1, get v() { return 7; } };
console.log("d", d.a, d.v);

// nested one level down
const e: { p: { v: number } } = { p: { get v() { return 7; } } };
console.log("e", e.p.v);

// as the element type of an array — the annotation must not be pushed
// onto the elements either, which is the same adoption by another route
const f: { v: number }[] = [{ get v() { return 7; } }, { get v() { return 8; } }];
console.log("f", f[0].v, f[1].v);

// an `any` annotation keeps winning: it admits the literal outright,
// so the projection never runs and the binding stays the box its
// dynamic member lane expects
const g: any = { get v() { return 7; } };
console.log("g", g.v);

// no annotation at all — the path the rest of these now join
const h = { get v() { return 7; } };
console.log("h", h.v);

// the getter still runs on every read, and still sees its own object
let calls = 0;
const i: { n: number } = { get n() { calls += 1; return calls; } };
console.log("i", i.n, i.n, i.n, calls);
