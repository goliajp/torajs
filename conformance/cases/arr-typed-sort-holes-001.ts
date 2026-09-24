// sort over a number[] with holes: present values sort, holes go to the
// tail (§23.1.3.30 SortIndexedProperties skips them); a stored
// undefined sorts after the numbers and before the holes.
function mk(): number[] {
  let a = [1.5, 2.5];
  a[4] = 3.5;
  return a;
}
console.log(mk().sort());
console.log(mk().sort((x, y) => y - x));
{ const a = mk(); a.sort((x, y) => x - y); console.log(a, 2 in a, 3 in a); }
const zs: number[] = [1.5];
{ const a = [3.5, 1.5]; a[1] = zs[9]; a[3] = 0.5; console.log(a.sort(), a.length); }
console.log(mk().toSorted());
const d = [3, 1, 2];
console.log(d.sort(), d.sort((x, y) => y - x));
const c: number[] = [1, 2];
Reflect.set(c, 0, undefined);
console.log(c[0], c);
// string[] / object[]: the holes are NULL slots; the comparator never
// sees one (it did — a SIGSEGV).
const s = ["b", "a"];
s[3] = "c";
console.log(s.sort(), 2 in s, 3 in s);
const s2 = ["b", "a"];
s2[3] = "c";
console.log(s2.sort((x, y) => (x < y ? 1 : -1)));
const o = [{ v: 2 }, { v: 1 }];
o[3] = { v: 0 };
console.log(o.sort((a, b) => a.v - b.v).map((e) => e.v), o.length, 3 in o);
const big: string[] = [];
for (let i = 0; i < 40; i++) big.push(String((i * 7) % 40));
big[45] = "zz";
big.sort();
console.log(big.length, big[0], big[39], big[40], 44 in big, 39 in big);
const t = ["b", "a"];
t[3] = "c";
try {
  t.sort((x: string, y: string): number => {
    throw new Error("boom");
  });
} catch (e) {
  console.log(String(e), t.length, 3 in t);
}
