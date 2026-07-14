// The any-lane iteration kernel (`__torajs_any_iter_next`) steps class
// instances too, not just strings / arrays / Map-Set iterator cells: a
// generator object or any user class with `[Symbol.iterator]()` held
// behind an `any` now iterates per ES 7.4.3 — GetIterator once, then
// `next()` per step, reading `done` / `value` off the IteratorResult.
// Both calls go through the class-methods dispatch table.
//
// The iterator the kernel derives is parked in a caller-owned slot for
// the life of the loop (re-deriving it per step would restart an
// iterable that mints a fresh iterator), and the loop's exit block
// releases it — which is also the block a `break` lands in.

function* count(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}

// Generator behind `any` — used to throw "value is not iterable".
const g: any = count();
let sum = 0;
for (const v of g) { sum = sum + v; }
console.log(sum); // 6

// Spread of an any-held generator goes through the same kernel.
const h: any = count();
const xs = [...h];
console.log(xs.join(",")); // 1,2,3

// break mid-loop: the parked iterator is released at the exit block.
const b: any = count();
const seen: any[] = [];
for (const v of b) {
  if (v === 3) break;
  seen.push(v);
}
console.log(seen.join(",")); // 1,2

// A user class iterable behind `any` resolves the same way.
class Range {
  cur: number;
  limit: number;
  constructor(limit: number) { this.cur = 0; this.limit = limit; }
  [Symbol.iterator](): Range { return this; }
  next(): IteratorResult<number> {
    const v = this.cur;
    this.cur = this.cur + 1;
    return { value: v, done: v >= this.limit };
  }
}
const r: any = new Range(4);
let acc = 0;
for (const v of r) { acc = acc + v; }
console.log(acc); // 0+1+2+3 = 6

// A non-iterable `any` still raises a catchable TypeError, and a
// plain object (no [Symbol.iterator]) is non-iterable too.
try {
  const n: any = 42;
  for (const v of n) { console.log(v); }
} catch (e) {
  console.log("caught number");
}
