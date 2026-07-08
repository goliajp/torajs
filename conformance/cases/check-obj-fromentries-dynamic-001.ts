// Chunk 693 — Object.fromEntries with a RUNTIME entries array
// (T-09, the dynamic complement to the chunk-690 static fold):
// __torajs_anyv_from_entries walks the pairs array kind-aware per
// slot (typed 8-byte blocks stamp their elem kind via
// arr_mark_kind before the call; FLAG_ARR_ANY blocks read the
// NaN-box slots) and builds a dynobj in insertion order — last
// duplicate wins, a short entry answers undefined, undefined /
// null / non-array receivers throw a catchable TypeError
// (probe-verified; the throw lanes stay out of this fixture as
// the error print shape differs from bun).
const pairs = [
  ["a", 1],
  ["b", 2],
];
const o = Object.fromEntries(pairs);
console.log(o.a, o.b);
// all-string pairs (typed Arr<Arr<Str>> source)
const strs = [
  ["k", "v"],
  ["m", "w"],
];
const so = Object.fromEntries(strs);
console.log(so.k, so.m);
// Object.entries round-trip without a struct annotation
const src = { x: 1.5, y: 2.5 };
const round = Object.fromEntries(Object.entries(src));
console.log(round.x, round.y);
// empty entries
const eo = Object.fromEntries([]);
console.log(eo);
// duplicate key — last wins
const dup = [
  ["a", 1],
  ["a", 9],
];
console.log(Object.fromEntries(dup).a);
// short entry — value undefined
const short: any[] = [["only"]];
console.log(Object.fromEntries(short).only);
