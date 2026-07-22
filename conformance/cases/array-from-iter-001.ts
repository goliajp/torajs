// Array.from over statically-typed Map / MapIter / ArrIter sources.
// The checker admits Map/MapIter/ArrIter as `Array.from` iter args
// (routing to `Array<Any>`); lowering boxes the heap source and drives
// the unified runtime iteration protocol (shares the materializer with
// `[...x]` spread's iterator arm).
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
console.log(Array.from(m.keys()));
console.log(Array.from(m.values()));
console.log(Array.from(m.entries()).length);
const all = Array.from(m);
console.log(all[0][0], all[0][1], all[1][0], all[1][1]);

const s = new Set<number>();
s.add(10);
s.add(20);
console.log(Array.from(s.values()));

// ArrIter: `arr.keys()` on a typed array yields ArrIter (indices)
const a = [7, 8, 9];
console.log(Array.from(a.keys()));

// 2-arg mapFn over an iterator source (elem is Any-erased)
console.log(Array.from(m.keys(), (k) => k.toUpperCase()));
console.log(Array.from(m.values(), (v) => v * 10));
console.log(Array.from(a.keys(), (i) => i * 2));

// owned-temp source: each m.keys() mints a fresh iterator, consumed
// once and dropped. Churn 1000x asserts no leak in the
// box -> materialize -> release_owned_temp path.
let total = 0;
for (let i = 0; i < 1000; i++) {
  const ks = Array.from(m.keys());
  total = total + ks.length;
}
console.log(total);
console.log("done");
