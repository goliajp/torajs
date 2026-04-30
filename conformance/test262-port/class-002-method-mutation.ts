// Adapted from test262: a class method writes to `this.field`. Verifies
// that the method's __this borrow is mutable in tr's desugar.
class Acc {
  total: number;
  constructor() {
    this.total = 0;
  }
  add(n: number): number {
    this.total = this.total + n;
    return this.total;
  }
}

function check(): number {
  let a = new Acc();
  if (a.add(10) !== 10) { throw "#1"; }
  if (a.add(20) !== 30) { throw "#2"; }
  if (a.add(-5) !== 25) { throw "#3"; }
  return 0;
}
console.log(check());
