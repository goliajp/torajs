// chunk 739 — Object.fromEntries accepts dynobj array-like entries
// (ES §20.1.2.7 AddEntriesFromIterable reads Get(entry, "0") /
// Get(entry, "1"); any Object is a legal entry, not just arrays)
const entry: any = { 0: "a", 1: 1 };
const o = Object.fromEntries([entry]);
console.log(o.a);

// short entry: absent "1" answers undefined
const short: any = { 0: "b" };
const p = Object.fromEntries([short]);
console.log(p.b);

// absent "0" stringifies to "undefined" via ToPropertyKey
const noKey: any = { 1: "only-val" };
const q = Object.fromEntries([noKey]);
console.log(q.undefined);

// mixed pair-array and dynobj entries; later duplicate key wins
// (an inline `{...} as any` literal boxes as an anon-struct, not a
// dynobj — struct-via-any entries stay loud, recorded narrow face)
const d1: any = { 0: "x", 1: "vx" };
const dup: any = { 0: "a", 1: 9 };
const m = Object.fromEntries([["a", 1] as any, d1, dup]);
console.log(m.x, m.a);

// Set of dynobj entries
const st = new Set<any>();
st.add(entry);
st.add(d1);
const r = Object.fromEntries(st);
console.log(r.a, r.x);

// primitive entry still throws (catchable)
try {
  Object.fromEntries([42 as any]);
} catch (err) {
  console.log("caught-prim");
}
