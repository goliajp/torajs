// Chunk 694 — Object.fromEntries with a Map / Set receiver (the
// hash-storage lanes over __torajs_map_iter_next; dynamic
// complement closes the fromEntries receiver family after the
// chunk-693 array walker). A Map entry is a (k, v) pair by
// construction (no validate pass); a Set element must itself be a
// pair array — a primitive element throws a catchable TypeError
// (probe-verified; the throw lane stays out of this fixture as
// the error print shape differs from bun). Keys ride
// ToPropertyKey (1 → "1", true → "true"); insertion order wins.
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
const o = Object.fromEntries(m);
console.log(o);
console.log(o.a + o.b);
// any-typed Map source (runtime tag dispatch inside the walker)
const am: any = new Map<string, string>();
am.set("x", "ex");
am.set("y", "wai");
const ao = Object.fromEntries(am);
console.log(ao.x, ao.y);
// non-string keys — ToPropertyKey (print side quotes "1", bares true)
const mk: any = new Map<any, any>();
mk.set(1, "one");
mk.set(true, "t");
console.log(Object.fromEntries(mk));
// empty Map
const em = new Map<string, number>();
console.log(Object.fromEntries(em));
// delete + re-set — tombstone skip keeps live insertion order
const dm = new Map<string, number>();
dm.set("first", 1);
dm.set("gone", 0);
dm.set("last", 9);
dm.delete("gone");
console.log(Object.fromEntries(dm));
// Set of pair arrays (the iterated element IS the entry)
const s = new Set<any>();
s.add(["p", 10]);
s.add(["q", 20]);
console.log(Object.fromEntries(s));
// heap values stay owned across the walk
const hm = new Map<string, string>();
hm.set("greet", "hello world");
const ho = Object.fromEntries(hm);
console.log(ho.greet.toUpperCase());
