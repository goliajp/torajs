// Iterator Helpers proposal 2.1.5 — lazy and eager helpers close the
// underlying iterator (call its return()) when the callback throws.
// The close runs under the stashed pending throw so the user
// return() body actually executes (IfAbruptCloseIterator).
class TestError extends Error {}
class TestIterator extends Iterator {
  closed: boolean = false;
  next(): any {
    return { done: false, value: 1 };
  }
  return(): any {
    this.closed = true;
    return { done: true };
  }
}
function boom(): any {
  throw new TestError();
}

// lazy helpers
const it1 = new TestIterator();
console.log(it1.closed);
try {
  it1.map(boom).next();
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}
console.log(it1.closed);
const it2 = new TestIterator();
try {
  it2.filter(boom).next();
} catch (e) {}
console.log(it2.closed);
const it3 = new TestIterator();
try {
  it3.flatMap(boom).next();
} catch (e) {}
console.log(it3.closed);

// eager consumers
const a = new TestIterator();
try {
  a.forEach(boom);
} catch (e) {}
console.log(a.closed);
const b = new TestIterator();
try {
  b.reduce(boom, 0);
} catch (e) {}
console.log(b.closed);
const c = new TestIterator();
try {
  c.some(boom);
} catch (e) {}
console.log(c.closed);

// flatMap non-flattenable mapped value closes under the TypeError
const d = new TestIterator();
try {
  d.flatMap((x: any) => 5).next();
} catch (e) {
  console.log("ft");
}
console.log(d.closed);
