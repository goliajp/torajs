// chunk 791 — `?.` on sentinel-capable non-Obj receivers. The
// optchain arm's nullish check now mirrors the chunk-786 `??`
// station (NULL or the receiver type's undefined sentinel), and the
// non-Obj hit path dispatches through the regular typed-receiver
// member ladder. Previously: a Str receiver panicked loudly
// ("optional chain on non-struct obj type Str"), and an optional
// struct-typed field holding the sentinel rode the hit path and
// loaded garbage past the sentinel header (silent wrong).
type O = { tag?: string; n: number };
const o: O = { n: 5 };
console.log(o.tag?.length);
console.log(o.tag?.length ?? -1);
const p: O = { tag: "abc", n: 1 };
console.log(p.tag?.length);
console.log(p.tag?.length ?? -1);

type Inner = { x: number };
type P = { child?: Inner; n: number };
const q: P = { n: 5 };
console.log(q.child?.x);
console.log(q.child?.x ?? -1);
const r: P = { child: { x: 7 }, n: 1 };
console.log(r.child?.x);
console.log(r.child?.x ?? -1);

const s: string | null = null;
console.log(s?.length);
console.log(s?.length ?? -1);
const t: string | null = "hey";
console.log(t?.length);

type F = { cb?: () => number; n: number };
const f: F = { n: 1 };
console.log(f.cb?.name);
console.log(f.cb?.length ?? -2);
// (filled `.name` deliberately not asserted: a closure read through
// a non-Ident receiver answers the pre-existing static "" name
// approximation — named-evaluation name repr is an L3b axis.)
const g = () => 42;
const h: F = { cb: g, n: 2 };
console.log(h.cb?.length ?? -2);

type A = { xs?: number[]; n: number };
const a: A = { n: 2 };
console.log(a.xs?.length);
console.log(a.xs?.length ?? -3);
const b: A = { xs: [1, 2, 3], n: 3 };
console.log(b.xs?.length);
