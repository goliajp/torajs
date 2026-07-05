// RFC 20260705 — expression-statement discard releases provably
// owned results: the Map/Set set()/add() chaining +1 and any-typed
// Call boxes. Containers stay fully usable after discarded calls.
const m = new Map<number, string>();
m.set(1, "a");
m.set(2, "b");
m.set(1, "c");
console.log(m.size);
console.log(m.get(1));

const s = new Set<number>();
s.add(10);
s.add(10);
s.add(20);
console.log(s.size);

// discarded any-world method results (owned boxes)
const ma: any = m;
ma.set(3, "d");
ma.get(1);
ma.keys();
console.log(ma.size);

// discarded any-typed user-fn results
function give(x: any): any {
  return x;
}
give(ma);
give("str-box");
console.log(m.size);

// set/add inside loops — the shape that accumulated leaked +1s
for (let i = 0; i < 100; i++) {
  const t = new Map<number, number>();
  t.set(i, i);
}
console.log("loop done");

// half-exhausted iterator: discarded next() keeps cursor semantics
const it: any = m.keys();
it.next();
for (const k of it) console.log("rest", k);
console.log("done");
