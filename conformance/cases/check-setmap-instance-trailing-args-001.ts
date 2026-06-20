// S264 — Set/Map instance method trailing-arg ignore per ES
// §24.2.3.{4,5,7} (Set.{delete,clear,has}) + §23.1.3.{3,4,6,7}
// (Map.{delete,clear,get,has}). Each method's fixed sig
// (vec![Any] / Vec::new()) rejected 1+ trailing args; tora widens
// at check.rs (carve-out) + ssa_lower.rs (debug-assert relaxed).
// Set.add / Map.set already covered by S248.
// Only console.log primitives (Boolean / Number / Nullable<Number>)
// to dodge Set/Map display format diff (prior carry).

const s = new Set<number>([1, 2, 3]);
console.log(s.has(1, 99));
console.log(s.has(2, 99, 88));
console.log(s.has(99, 77));
console.log(s.delete(2, 99));
console.log(s.delete(99, 77, 66));
console.log(s.size);
s.clear(99);
console.log(s.size);
s.clear(99, 88, 77);
console.log(s.size);

const m = new Map<string, number>([["a", 1], ["b", 2]]);
console.log(m.has("a", 99));
console.log(m.has("z", 77, 66));
console.log(m.get("a", 99));
const miss = m.get("z", 77, 66);
console.log(miss === undefined);
console.log(m.delete("a", 99));
console.log(m.delete("z", 77, 66));
console.log(m.size);
m.clear(99);
console.log(m.size);
m.clear(99, 88, 77);
console.log(m.size);
