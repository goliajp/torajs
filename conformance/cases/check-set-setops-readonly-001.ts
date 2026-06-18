// Set read-only setops per ES2025 §24.2.3.{12,13,14}:
//   isSubsetOf / isSupersetOf / isDisjointFrom.

const a = new Set<number>([1, 2, 3]);
const b = new Set<number>([2, 3, 4]);
const c = new Set<number>([1, 2]);
const d = new Set<number>([5, 6]);
const empty = new Set<number>();

// isSubsetOf
console.log("a.isSubsetOf(b)", a.isSubsetOf(b));
console.log("c.isSubsetOf(a)", c.isSubsetOf(a));
console.log("a.isSubsetOf(a)", a.isSubsetOf(a));
console.log("empty.isSubsetOf(a)", empty.isSubsetOf(a));
console.log("a.isSubsetOf(empty)", a.isSubsetOf(empty));
console.log("empty.isSubsetOf(empty)", empty.isSubsetOf(empty));

// isSupersetOf
console.log("a.isSupersetOf(c)", a.isSupersetOf(c));
console.log("a.isSupersetOf(b)", a.isSupersetOf(b));
console.log("a.isSupersetOf(a)", a.isSupersetOf(a));
console.log("a.isSupersetOf(empty)", a.isSupersetOf(empty));
console.log("empty.isSupersetOf(a)", empty.isSupersetOf(a));
console.log("empty.isSupersetOf(empty)", empty.isSupersetOf(empty));

// isDisjointFrom
console.log("a.isDisjointFrom(d)", a.isDisjointFrom(d));
console.log("a.isDisjointFrom(b)", a.isDisjointFrom(b));
console.log("a.isDisjointFrom(a)", a.isDisjointFrom(a));
console.log("empty.isDisjointFrom(a)", empty.isDisjointFrom(a));
console.log("a.isDisjointFrom(empty)", a.isDisjointFrom(empty));

// String-keyed Sets (heap keys via hash_bytes path)
const xa = new Set<string>(["apple", "banana", "cherry"]);
const xb = new Set<string>(["banana", "cherry"]);
console.log("str.xb.isSubsetOf(xa)", xb.isSubsetOf(xa));
console.log("str.xa.isSubsetOf(xb)", xa.isSubsetOf(xb));
console.log("str.xa.isDisjointFrom(xb)", xa.isDisjointFrom(xb));

// After delete — verify entries with hash=0 are skipped
const m = new Set<number>([10, 20, 30, 40]);
m.delete(20);
m.delete(40);
const m_target = new Set<number>([10, 30]);
console.log("after-delete.m.isSubsetOf(m_target)", m.isSubsetOf(m_target));
console.log("after-delete.m_target.isSubsetOf(m)", m_target.isSubsetOf(m));

// Single-element Sets
const single = new Set<number>([42]);
const single_dup = new Set<number>([42]);
console.log("single.isSubsetOf(single_dup)", single.isSubsetOf(single_dup));
console.log("single.isDisjointFrom(single_dup)", single.isDisjointFrom(single_dup));
