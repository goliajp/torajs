// RFC 20260704 C4-2 — `.size` member read + Map/Set forEach through
// `any` receivers.
const m: any = new Map();
m.set("a", 1);
m.set("b", 2);
console.log(m.size);
const s: any = new Set();
s.add(10);
s.add(20);
s.add(10);
console.log(s.size);
const o: any = { size: 42 };
console.log(o.size);
const a: any = [1, 2, 3];
console.log(a.size);
const str: any = "hi";
console.log(str.size);
m.forEach((v: any, k: any, mm: any) => {
  console.log(k, v, mm.size);
});
s.forEach((v: any, k: any, ss: any) => {
  console.log(v, k, v === k, ss.size);
});
m.delete("a");
console.log(m.size);
m.clear();
console.log(m.size);
