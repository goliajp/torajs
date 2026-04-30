// Adapted from test262: language/statements/class/definition/basics.js
// Single class — fields + constructor + a method that reads `this`.
class Counter {
  value: number;
  constructor(initial: number) {
    this.value = initial;
  }
  get(): number { return this.value; }
}

function check(): number {
  let c = new Counter(5);
  if (c.get() !== 5) { throw "#1"; }
  let d = new Counter(0);
  if (d.get() !== 0) { throw "#2"; }
  return 0;
}
console.log(check());
