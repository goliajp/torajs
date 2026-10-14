// a method twin entered through a dynamic call writes what the typed
// method could not: the member it writes reads that value back
class Builder {
  v: number;
  constructor() { this.v = 0; }
  add(n: number): void { this.v = this.v + n; }
}
const b = new Builder();
b.add(3);
const a: any = b;
a.add("x");
console.log(b.v, typeof b.v);

class Steps {
  i: number = 0;
  last: number = -1;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i, done: this.i > 3 };
  }
  return(v: number): { value: number; done: boolean } {
    this.last = v;
    return { value: v, done: true };
  }
  [Symbol.iterator]() { return this; }
}
const s = new Steps();
for (const x of s) {
  if (x === 2) break;
}
console.log(s.i, s.last, typeof s.last);

class Counter {
  n: number = 1;
  bump(k: number): number { this.n = this.n * k; return this.n; }
}
const c = new Counter();
c.bump(2);
const f = c.bump;
(f as any).call(c, "3");
console.log(c.n, typeof c.n);
