// Array spread over STATICALLY-TYPED Map / MapIter sources (no `any`
// annotation on the source). The checker admits Map/MapIter/ArrIter
// as spread sources routing to `Array<Any>`; lowering boxes the heap
// source and drives the unified runtime iteration protocol (mirrors
// the `any`-typed sibling any-spread-iter-001).
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
console.log([...m.keys()]);
console.log([...m.values()]);
const es = [...m.entries()];
console.log(es[0][0], es[0][1]);
console.log(es[1][0], es[1][1]);
console.log([...m].length);
console.log([...m][0][0], [...m][0][1]);

// statically-typed Set: `[...s]` (existing arm) + `[...s.values()]`
// (MapIter, new arm) — regression + new capability together
const s = new Set<number>();
s.add(10);
s.add(20);
console.log([...s]);
console.log([...s.values()]);

// literal + iterator-spread mix
console.log([0, ...m.keys(), 99]);

// owned-temp source: the iterator minted by the call is consumed once
// and dropped — no leak, minted exactly once
let calls = 0;
const mk = () => {
  calls = calls + 1;
  return m.keys();
};
console.log([...mk()]);
console.log(calls);

// churn — repeated statically-typed iterator spread must not leak
let total = 0;
for (let i = 0; i < 1000; i++) {
  const ks = [...m.keys()];
  total = total + ks.length;
}
console.log(total);
console.log("done");
