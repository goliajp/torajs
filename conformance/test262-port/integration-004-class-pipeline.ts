// Integration: class with array field + methods that mutate state
// across iterations. Exercises class field initialization, struct-
// field push, method dispatch, and the for-of iteration over the
// class's stored array.
class Tally {
  values: number[];
  total: number;
  count: number;
  constructor() {
    let init: number[] = [];
    this.values = init;
    this.total = 0;
    this.count = 0;
  }
  add(n: number): void {
    this.values.push(n);
    this.total = this.total + n;
    this.count = this.count + 1;
  }
  average(): f64 {
    if (this.count === 0) { return 0.0; }
    return this.total / this.count;
  }
  max(): number {
    if (this.count === 0) { return 0; }
    let m = this.values[0];
    for (let v of this.values) {
      if (v > m) { m = v; }
    }
    return m;
  }
  min(): number {
    if (this.count === 0) { return 0; }
    let m = this.values[0];
    for (let v of this.values) {
      if (v < m) { m = v; }
    }
    return m;
  }
}

function check(): number {
  let t = new Tally();
  if (t.count !== 0) { throw "#1: empty count"; }
  if (t.total !== 0) { throw "#2"; }
  if (t.average() !== 0) { throw "#3: avg of empty"; }

  t.add(10);
  t.add(20);
  t.add(30);
  if (t.count !== 3) { throw "#4"; }
  if (t.total !== 60) { throw "#5"; }
  if (t.average() !== 20) { throw "#6"; }
  if (t.max() !== 30) { throw "#7"; }
  if (t.min() !== 10) { throw "#8"; }

  // Add a few more.
  t.add(5);
  t.add(100);
  t.add(15);
  if (t.count !== 6) { throw "#9"; }
  if (t.max() !== 100) { throw "#10"; }
  if (t.min() !== 5) { throw "#11"; }

  // Array len matches count.
  if (t.values.length !== 6) { throw "#12"; }
  return 0;
}
console.log(check());
