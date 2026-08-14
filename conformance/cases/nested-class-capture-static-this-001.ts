// A nested class that reads an outer local rides the ES5 lane, and a
// static method's `this` is the class object on that lane too — the
// same object ES §10.2.1.2 binds when `K.s()` is called.
function run(base: number) {
  class Counter {
    n: number;
    constructor(start: number) {
      this.n = start + base;
    }
    static unit() {
      return base;
    }
    static twice() {
      return this.unit() * 2;
    }
    static viaArrow() {
      const f = () => this.unit();
      return f();
    }
    static self() {
      return this;
    }
    bump() {
      return this.n + 1;
    }
  }
  const out: any[] = [];
  out.push(Counter.unit());
  out.push(Counter.twice());
  out.push(Counter.viaArrow());
  out.push(Counter.self() === Counter);
  out.push(new Counter(1).bump());
  return out;
}

console.log(run(10).join(","));
console.log(run(100).join(","));
