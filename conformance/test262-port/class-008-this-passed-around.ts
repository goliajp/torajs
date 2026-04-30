// Adapted from test262: a class method passes `this` to another method.
// Verifies the desugared receiver-borrow plumbing across method-to-method
// invocation chains.
class Counter {
  value: number;
  constructor(v: number) { this.value = v; }
  add(n: number): number {
    this.value = this.value + n;
    return this.value;
  }
  addTwice(n: number): number {
    this.add(n);
    return this.add(n);
  }
}

function check(): number {
  let c = new Counter(0);
  if (c.addTwice(5) !== 10) { throw "#1"; }
  if (c.value !== 10) { throw "#2"; }
  if (c.addTwice(1) !== 12) { throw "#3"; }
  return 0;
}
console.log(check());
