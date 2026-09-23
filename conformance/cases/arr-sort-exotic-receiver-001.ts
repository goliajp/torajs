// §23.1.3.30 SortIndexedProperties: holes and accessor indices go
// through HasProperty / Get / Set / Delete, not a raw slot permutation.
const array = [undefined, 'c', /*hole*/, 'b', undefined, /*hole*/, 'a', 'd'];
Object.defineProperty(array, '2', {
  get() { array[3] = 'foobar'; return (this as any).foo; },
  set(v) { (this as any).foo = v; }
});
array.sort();
console.log(array[0], array[1], array[2], array[3], array[4], array[5], array[6], '7' in array, array.length, (array as any).foo);
const b: any = [3, , 1];
b.sort();
console.log(b.length, 0 in b, 1 in b, 2 in b, b[0], b[1]);
const c = [5, , 4, undefined, 1];
const r = c.sort((x: any, y: any) => x - y);
console.log(r === c, c.length, 3 in c, 4 in c, c[0], c[1], c[2], c[3]);
const d: any[] = ["b", , "a"];
console.log(d.sort().join("|"), d.length, 2 in d);
const e = [3, 1, 2];
console.log(e.sort().join());
let calls = 0;
const f: any = [2, , 1];
try { f.sort(() => { calls++; throw new Error("x"); }); } catch (err: any) { console.log("threw", err.message, calls); }
