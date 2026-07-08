// JSON.stringify(NULL Obj slot) answers "null" per §25.5.2 instead
// of dereferencing NULL on the first field load — the obj-lane
// mirror of the 655 arr-lane null gate (the union-annotation parse
// reject that used to shield this lane is gone).

// jsb fast path (primitive-only layout) holding null
const o: { a: number } | null = null;
console.log(JSON.stringify(o));

// jsb fast path holding a value — regression lane
const p: { a: number } | null = { a: 1 };
console.log(JSON.stringify(p));

// str_concat slow path (F64 field forces it) — both states
const q: { x: number; s: string } | null = null;
console.log(JSON.stringify(q));
const r: { x: number; s: string } | null = { x: 1.5, s: "y" };
console.log(JSON.stringify(r));

// alias of a null obj binding
const alias = o;
console.log(JSON.stringify(alias));
