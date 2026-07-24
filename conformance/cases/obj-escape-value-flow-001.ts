// 11-A2-a obj-escape: aliases behind value-transparent wrappers
// (ternary return / as-cast return / member-store of a ternary /
// recursion) must mark the source object escape-bound so it heap-
// allocs. Regression guard for the bare-Ident-only escape check
// (deque sibling had an observable wrong-offset read; this face is
// hardened to the same contract).
function makeTernary(): { v: number } {
  const o = { v: 42 };
  return true ? o : o;
}
function makeAs(): { v: number } {
  const o = { v: 7 };
  return o as { v: number };
}
function storeTernary(): number {
  const o = { v: 5 };
  const box = { a: { v: 0 } };
  box.a = true ? o : o;
  return box.a.v;
}
function make(n: number): { v: number } {
  const o = { v: n };
  if (n <= 0) {
    return true ? o : o;
  }
  const prev = make(n - 1);
  return true ? o : prev;
}
console.log(makeTernary().v);
console.log(makeAs().v);
console.log(storeTernary());
const r = make(5);
const r2 = make(9);
console.log(r.v);
console.log(r2.v);
