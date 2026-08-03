// GetIteratorDirect caches the underlying's next method at helper
// construction (§27.1.4.x step 4 / §27.1.6.2) — the accessor fires
// ONCE per helper, not once per step. Mirrors test262's
// get-next-method-only-once family (8 cases were tr-timeout: the
// per-step re-Get minted a fresh generator each step and the walk
// never terminated).
let nextGets = 0;
let nextCalls = 0;

class CountingIterator extends Iterator {
  get next() {
    ++nextGets;
    const iter = (function* () {
      for (let i = 1; i < 5; ++i) {
        yield i;
      }
    })();
    return function () {
      ++nextCalls;
      return iter.next();
    };
  }
}

// lazy map driven by for-of
for (const value of new CountingIterator().map((x: any) => x * 10)) {
  console.log(value);
}
console.log(nextGets, nextCalls);

// eager toArray straight off the receiver
nextGets = 0;
nextCalls = 0;
console.log(new CountingIterator().toArray());
console.log(nextGets, nextCalls);

// eager reduce (three-arg callback lane)
nextGets = 0;
nextCalls = 0;
console.log(new CountingIterator().reduce((a: any, b: any) => a + b, 0));
console.log(nextGets, nextCalls);

// drop's ahead-of-first-step skip loop rides the same cached next
nextGets = 0;
nextCalls = 0;
console.log(new CountingIterator().drop(2).toArray());
console.log(nextGets, nextCalls);

// flatMap's outer step
nextGets = 0;
nextCalls = 0;
console.log(new CountingIterator().flatMap((x: any) => [x]).toArray());
console.log(nextGets, nextCalls);

// filter chained onto map — each helper caches its own source's next
nextGets = 0;
nextCalls = 0;
console.log(
  new CountingIterator()
    .map((x: any) => x + 1)
    .filter((x: any) => x % 2 === 0)
    .toArray()
);
console.log(nextGets, nextCalls);
