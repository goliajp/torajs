// RFC 20260714-dstr-residual blade 3 — the iterator walk a destructuring
// pattern drives is BOUNDED, and it closes what it doesn't exhaust.
//
// A pattern names a fixed number of elements, so it steps exactly that
// many times (ES §13.15.5.3). Draining the source instead — the shape a
// plain `[...src]` spread would have given — is observably wrong three
// ways, and this fixture pins all three.

// 1. It would never terminate on an infinite iterator.
function* nat() {
  let i = 0;
  while (true) {
    yield i++;
  }
}
const [x, y, z] = nat();
console.log(x, y, z);

// 2. It would run side effects the pattern never asked for: `s3` is
//    past the budget of a two-element pattern and must not print.
function* loud() {
  console.log("s1");
  yield 1;
  console.log("s2");
  yield 2;
  console.log("s3");
  yield 3;
}
const [m, n] = loud();
console.log(m, n);

// 3. Stopping short leaves the iterator live, and ES §7.4.9 owes it an
//    IteratorClose — the `return()` call that lets it clean up. It runs
//    BEFORE the bindings are used.
class It {
  i: number = 0;
  [Symbol.iterator](): any {
    return this;
  }
  next(): any {
    this.i = this.i + 1;
    return { value: this.i, done: this.i > 5 };
  }
  return(): any {
    console.log("closed at", this.i);
    return { value: 0, done: true };
  }
}
const [first, next] = new It();
console.log(first, next);

// An exhausted iterator is NOT closed — the rest element drained it, so
// no "closed at" line follows.
const [head, ...tail] = new It();
console.log(head, tail.length);

// A pattern that binds nothing still calls GetIterator, so a
// non-iterable source is a TypeError even with zero reads.
try {
  const [] = 5;
  console.log("no throw");
} catch (err) {
  console.log("caught", err.name);
}
try {
  const [only] = { a: 1 };
  console.log("no throw", only);
} catch (err) {
  console.log("caught", err.name);
}
const [] = [1, 2];
console.log("empty over array ok");
