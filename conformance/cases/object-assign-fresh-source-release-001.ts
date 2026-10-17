// a fresh object-literal source is released after its members are copied
// onto a struct target; the copied members keep their own references
type T = { a: number; s: string; o: { n: number } };
const t: T = { a: 0, s: "", o: { n: 0 } };
const keep: { n: number }[] = [];
for (let i = 0; i < 200000; i++) {
  const inner = { n: i };
  Object.assign(t, { a: i, s: "v" + i, o: inner });
  if (i % 50000 === 0) keep.push(t.o);
}
console.log(t.a, t.s, t.o.n, keep.length, keep[3].n);
const r = Object.assign(t, { a: 1, s: "x", o: { n: 2 } }, { a: 3, s: "y", o: { n: 4 } });
console.log(r === t, t.a, t.s, t.o.n);
