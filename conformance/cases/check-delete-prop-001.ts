// chunk 813 — `delete` operator (ES §13.5.1) on any receivers:
// dynobj OrdinaryDelete / expando props / non-object true; RFC
// 20260711-delete-forin-propertyhelper chunk A.
const o: any = { x: 7, y: "hi" };
console.log(delete o.x);
console.log(o.x);
console.log(Object.keys(o));

// absent key and literal-index spelling answer true
const p: any = { x: 7 };
console.log(delete p.absent);
console.log(delete p["x"]);

// dynamic string key
const q: any = { z: 1 };
const k: string = "z";
console.log(delete q[k], q.z);

// delete-then-set moves the key to the end (§10.1.10 + insertion order)
const r: any = { a: 1, b: 2, c: 3 };
delete r.b;
r.b = 9;
console.log(Object.keys(r));
console.log(r.b);

// array / closure expandos delete through the props dynobj
const arr: any = [1, 2];
arr.tag = "t";
console.log(delete arr.tag, arr.tag);
const f: any = (x: number) => x;
f.meta = 9;
console.log(delete f.meta, f.meta);

// non-object receiver answers true
console.log(delete (42 as any).x);

// null receiver throws a catchable TypeError
const n: any = null;
try { delete n.x; } catch (e) { console.log("caught"); }

// Map / Set `.delete` method names survive the keyword
const m = new Map<string, number>();
m.set("mk", 1);
console.log(m.delete("mk"), m.size);
const s = new Set<number>();
s.add(3);
console.log(s.delete(3), s.size);
