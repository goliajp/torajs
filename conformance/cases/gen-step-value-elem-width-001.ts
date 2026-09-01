// 553-04 — `for (const v of h(3))` binds `v` to the ELEMENT of the
// generator object, which the parser's pre-built `src[i]` element read
// spells as `Elem(Ret(h))`. The value actually delivered is the
// `value` field of what the desugared `__Gen_h` class answers from
// `next()`, and nothing joined the two classes — so a widened step
// value met a narrow `nums` at `nums.push(v)` and the write refused
// loudly ("container width analysis missed this write"). Collecting
// through string concatenation instead of push hid it, and so did
// `console.log(v)`: only a write into a typed array asks the question.
type Nums = number[];
const twice = (n: number): Nums => [n, n];
const loose = (n: number) => n + 1;
function named(n: number): string {
  return "n" + n;
}

function* h(k: number): Generator<number> {
  const xs = twice(k);
  yield xs[0];
  const m = loose(k);
  yield m;
  const nm = named(k);
  yield nm.length;
}

const nums: number[] = [];
for (const v of h(3)) {
  nums.push(v);
}
console.log(nums.join(","));

// the same collection through a nested generator delegating with
// yield*, so the step class is reached one hop further out
function* outer(k: number): Generator<number> {
  yield* h(k);
  yield 99;
}
const more: number[] = [];
for (const v of outer(2)) {
  more.push(v);
}
console.log(more.join(","));
