// Rotation 543 — both any-lane `next()` arms rc_inc'd the payload the
// step handed them, on a contract that had not been true since
// rotation 323.
//
// `__torajs_map_iter_step`'s BODY says it in as many words — "a heap
// one leaves here already +1'd, the entry keeps its own stake" — and
// its ENTRIES arms answer a `make_pair_arr` at refcount 1.
// `__torajs_arr_iter_step` says the same on every arm. Only the map
// step's HEADER doc still carried the old borrowed-payload contract,
// and both `*_iter_method` arms were written against the header
// rather than the body. `__torajs_dynobj_set` incs the KEY and
// nothing else, so the value slot adopts what it is handed: the inc
// was one too many and stranded a reference per yielded heap value.
//
// The typed lowering is the third witness. `ssa_lower_call_iter_next`
// calls the same steps and boxes the payload with no inc at all,
// which is why `xs.values().next()` was flat the whole time while the
// same iterator reached through the any lane leaked.
//
// 200k churn, AOT product RSS, 1.51 MB flat baseline:
//   m.entries().next()  via any   40.58 MB -> 2.06 MB
//   xs.entries().next() via any   34.11 MB -> 2.03 MB
//   xs[Symbol.iterator]().next()   8.67 MB -> 2.28 MB
//   m.values() / m.keys() .next()  8.47 MB -> 2.06 MB
//   "abc"[Symbol.iterator]().next()  8.52 MB -> 2.15 MB
//
// What the gate CAN see is the opposite failure. Removing an inc is
// the direction that produces use-after-free, so every case below
// keeps the yielded value alive past the death of both the iterator
// and the source it came from.
function pull(): any {
  const a = ["x" + 1];
  const it = a[Symbol.iterator]();
  return it.next().value;
}
const v = pull();
for (let j: number = 0; j < 200; j++) {
  const junk = ["z" + j];
}
console.log(v, v.length);

function pullEntry(): any {
  const a = ["x" + 1, "y" + 2];
  const it: any = a.entries();
  return it.next().value;
}
const e = pullEntry();
for (let j: number = 0; j < 200; j++) {
  const junk = ["z" + j];
}
console.log(e, e.length, e[1]);

function pullChar(): any {
  const s = "ab" + "c";
  const it = s[Symbol.iterator]();
  return it.next().value;
}
const c = pullChar();
for (let j: number = 0; j < 200; j++) {
  const junk = ["z" + j];
}
console.log(c, c.length);

function pullMapValue(): any {
  const m = new Map();
  m.set("k" + 1, "v" + 1);
  const it: any = m.values();
  return it.next().value;
}
const mv = pullMapValue();
for (let j: number = 0; j < 200; j++) {
  const junk = ["z" + j];
}
console.log(mv, mv.length);

const a2 = ["p", "q"];
const it2 = a2[Symbol.iterator]();
console.log(it2.next().value, it2.next().value, it2.next().done, a2);

const m2 = new Map();
m2.set("k", "v");
const it3: any = m2.entries();
const r3 = it3.next();
console.log(r3.value, r3.done, m2.get("k"), m2.size);

console.log([..."abc"], [...["a", "b"]]);
