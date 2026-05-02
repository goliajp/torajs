// Phase I.1 — verifies the iterator protocol (a class with
// `next(): { value: T, done: boolean }` returning a fresh struct each
// step) works on tr's existing class infrastructure. The compiler
// changes that landed alongside this port:
//   - desugar lifts the sibling-collision panic so unrelated classes
//     can share a method name (`.next` here).
//   - check.rs resolves `obj.M()` Member calls on Type::Struct against
//     the class's `__cm_<C>__M` signature when M isn't a struct field.
//   - ssa_lower routes those Member calls to the matching per-class
//     `__cm_<C>__<M>` static call (sibling dispatch — distinct from the
//     in-chain `__dispatch_<M>` runtime tag dispatcher).
// Locks the protocol shape ahead of Phase J (generator state machines)
// and Phase I.2 (for-of integration over user iterators).
type IterStep = { value: number, done: boolean };

class RangeIter {
  cur: number;
  end: number;
  constructor(s: number, e: number) {
    this.cur = s;
    this.end = e;
  }
  next(): IterStep {
    if (this.cur >= this.end) {
      return { value: 0, done: true };
    }
    let v = this.cur;
    this.cur = this.cur + 1;
    return { value: v, done: false };
  }
}

// A second, unrelated iterator class — exercises the sibling dispatch
// path (both classes have `next` but no inheritance relation).
class CountdownIter {
  remaining: number;
  constructor(n: number) {
    this.remaining = n;
  }
  next(): IterStep {
    if (this.remaining <= 0) {
      return { value: 0, done: true };
    }
    let v = this.remaining;
    this.remaining = this.remaining - 1;
    return { value: v, done: false };
  }
}

function check(): number {
  // Sum [0, 4) — basic protocol drive.
  let it = new RangeIter(0, 4);
  let total: number = 0;
  while (true) {
    let step = it.next();
    if (step.done) { break; }
    total = total + step.value;
  }
  if (total !== 6) { throw "#1: range sum"; }

  // Empty range — first call returns done immediately.
  let empty = new RangeIter(5, 5);
  let first = empty.next();
  if (first.done !== true) { throw "#2: empty done"; }

  // Drain twice — second drain on exhausted iterator stays done.
  let it2 = new RangeIter(0, 2);
  let s1 = it2.next(); if (s1.value !== 0 || s1.done !== false) { throw "#3"; }
  let s2 = it2.next(); if (s2.value !== 1 || s2.done !== false) { throw "#4"; }
  let s3 = it2.next(); if (s3.done !== true) { throw "#5: end"; }
  let s4 = it2.next(); if (s4.done !== true) { throw "#6: re-end"; }

  // Sibling-class iterator — different method body, same `.next` name.
  // Verifies tr resolves Member-call by obj's static class.
  let cd = new CountdownIter(3);
  let collected: number = 0;
  while (true) {
    let step = cd.next();
    if (step.done) { break; }
    collected = collected * 10 + step.value;
  }
  if (collected !== 321) { throw "#7: countdown"; }

  // Two iterators of the same type are independent.
  let a = new RangeIter(10, 13);
  let b = new RangeIter(10, 13);
  if (a.next().value !== 10) { throw "#8: a-0"; }
  if (a.next().value !== 11) { throw "#9: a-1"; }
  if (b.next().value !== 10) { throw "#10: b independent"; }
  if (a.next().value !== 12) { throw "#11: a-2"; }

  return 0;
}
console.log(check());
