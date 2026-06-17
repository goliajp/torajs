// S141 — Array.from(Set) per ES §23.1.2.1 + §24.2.3.13 (Set iterator
// protocol yields values). tr returns Array<Any> (Set storage is
// untyped); print path unboxes each tag-dispatched slot. Typed
// annotations (`const a: number[] = ...`) flow through the Array<Any>
// → Array<T> silent-coerce wedge (handoff 134 typed-Any wedge L3b),
// so this fixture uses the untyped binding form that matches the
// canonical ES idiom `Array.from(new Set(xs))`.

// number elem — dedup primitive
const s1 = new Set([1, 2, 2, 3, 1]);
const a1 = Array.from(s1);
console.log("len:", a1.length);
for (const x of a1) console.log(x);

// string elem — heap path goes through any_payload_rc_inc
const s2 = new Set(["a", "b", "a", "c"]);
const a2 = Array.from(s2);
console.log("len:", a2.length);
for (const x of a2) console.log(x);

// empty Set
const s3 = new Set<number>();
const a3 = Array.from(s3);
console.log("len:", a3.length);
