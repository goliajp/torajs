// §24.1.3.9 Map.prototype.set / §24.2.3.1 Set.prototype.add: an absent
// argument is `undefined`. The typed lowering indexed `args[1]` /
// `args[0]` outright, so `other.set(42)` (test262 set-methods, a Map
// standing in as the set-like) killed the compiler with an index-out-
// of-bounds (rotation 651).
const m = new Map();
m.set(42);
m.set();
console.log(m.size, m.get(42), m.has(undefined), m.get(undefined), m.get());
m.set(1, "one");
console.log(JSON.stringify([...m.entries()]));
const s = new Set();
s.add();
s.add(7);
console.log(s.size, s.has(undefined), JSON.stringify([...s]));
const first = new Set([42, 43]);
const other = new Map();
other.set(42);
other.set(46);
console.log(JSON.stringify([...first.intersection(other)]));
