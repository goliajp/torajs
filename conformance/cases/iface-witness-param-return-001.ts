// An interface as a parameter and as a return type: the callee calls
// through whatever instance it is handed.
interface Counter {
  inc(by: number): number;
  value(): number;
}
class Box {
  n: number;
  constructor() {
    this.n = 0;
  }
  inc(by: number): number {
    this.n += by;
    return this.n;
  }
  value(): number {
    return this.n;
  }
}
class Wrap {
  inner: Box;
  k: number;
  constructor() {
    this.inner = new Box();
    this.k = 10;
  }
  inc(by: number): number {
    return this.inner.inc(by * this.k);
  }
  value(): number {
    return this.inner.value();
  }
}
function bump(c: Counter, times: number): number {
  for (let i = 0; i < times; i++) c.inc(1.5);
  return c.value();
}
function pick(flag: boolean): Counter {
  if (flag) return new Box();
  return new Wrap();
}
console.log(bump(new Box(), 3), bump(new Wrap(), 2));
const p = pick(false);
p.inc(2);
console.log(p.value());
const q: Counter = pick(true);
console.log(bump(q, 4));
const lit: Counter = {
  inc(by: number) {
    return by * 100;
  },
  value() {
    return -1;
  },
};
console.log(bump(lit, 2));
