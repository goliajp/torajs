// ES §24.2.4.7 Set.prototype.intersection — result order follows
// the smaller side (§step 5.a iterates this if size ≤ other.size,
// step 5.b iterates other otherwise). Regression fix:
// __torajs_set_intersection unconditionally iterated `this`, so
// `new Set([3,2,1,0]).intersection(new Set([1,3,5]))` answered
// [3, 1] instead of the spec [1, 3], tripping test262
// Set/prototype/intersection/result-order.js.

// equal size — iterate this (this.size ≤ other.size).
{
  const a = new Set([1, 2, 3]);
  const b = new Set([2, 3, 4]);
  console.log([...a.intersection(b)]);
  // expected: [2, 3]
}

// this.size < other.size — iterate this.
{
  const a = new Set([3, 2, 1]);
  const b = new Set([1, 3, 5, 7]);
  console.log([...a.intersection(b)]);
  // expected: [3, 1]
}

// this.size > other.size — iterate other (the fix).
{
  const a = new Set([3, 2, 1, 0]);
  const b = new Set([1, 3, 5]);
  console.log([...a.intersection(b)]);
  // expected: [1, 3]
}

{
  const a = new Set([1, 3, 5, 7]);
  const b = new Set([3, 2, 1]);
  console.log([...a.intersection(b)]);
  // expected: [3, 1]  (iterate b = [3,2,1] insertion order, keep [3,1])
}

// empty other → empty result (short-circuit path preserved).
{
  const a = new Set([1, 2, 3]);
  const e = new Set<number>();
  console.log([...a.intersection(e)]);
}

// empty this → empty result (short-circuit path preserved).
{
  const e = new Set<number>();
  const b = new Set([1, 2, 3]);
  console.log([...e.intersection(b)]);
}

// disjoint — no members in common, iteration side irrelevant.
{
  const a = new Set([1, 2]);
  const b = new Set([3, 4, 5]);
  console.log([...a.intersection(b)]);
}

// self intersection — returns clone in this's order (this.size ≤ other.size).
{
  const a = new Set([5, 3, 1]);
  const b = new Set([5, 3, 1]);
  console.log([...a.intersection(b)]);
  // expected: [5, 3, 1]
}
