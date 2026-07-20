// §20.1.2.2 step 3 — a typed struct props (As-cast pass-through or a
// plain binding routed through any) must reach the
// ObjectDefineProperties walk: the struct operand IS the cell the
// kernel TAG_OBJ arm walks.
const p = { x: { value: 7, enumerable: true } };
const o = Object.create(null, p as any);
console.log((o as any).x);
const q: any = { y: { value: "s", enumerable: true } };
const o2 = Object.create(null, q);
console.log(o2.y);
const r = { a: { value: 1 }, b: { value: 2 } };
const o3 = Object.create({}, r as any);
console.log((o3 as any).a, (o3 as any).b);
