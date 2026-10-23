// a for-of over a class's own iterator binds the `value` its `next()`
// answers: the binding, and every container it is written into, take
// that value's width (here fractional) rather than a default one
class Halves {
  n: number = 0;
  [Symbol.iterator]() { return this; }
  next(): { value: number; done: boolean } {
    this.n = this.n + 1;
    return this.n <= 2 ? { value: this.n + 0.5, done: false } : { value: 0, done: true };
  }
}
class Counter {
  i: number = 0;
  end: number;
  constructor(end: number) { this.end = end; }
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return this.i <= this.end ? { value: this.i / 4, done: false } : { value: 0, done: true };
  }
}
class Range {
  end: number;
  constructor(end: number) { this.end = end; }
  [Symbol.iterator](): Counter { return new Counter(this.end); }
}
// a field read through a computed key may be any field, so every
// declared field holds any number, `n` below included
class Ints {
  n: number = 0;
  [Symbol.iterator]() { return this; }
  next(): { value: number; done: boolean } {
    this.n = this.n + 1;
    return this.n <= 2 ? { value: this.n, done: false } : { value: 0, done: true };
  }
}
class Prim {
  [Symbol.toPrimitive](hint: string) { return 42; }
}
const p: any = new Prim();
console.log(p[Symbol.toPrimitive]("number"));

const a: number[] = [];
for (const v of new Halves()) { a.push(v); }
console.log(a.join(","));

const h = new Halves();
const b: number[] = [];
for (const v of h) { b.unshift(v); }
console.log(b.join(","));

let last = 0;
for (const v of new Range(3)) { last = v; }
console.log(last);

let c: number[] = [];
for (const v of new Range(2)) { c = [v]; }
console.log(c.join(","));

const it = new Ints();
const d: number[] = [];
for (const v of it) { d.push(v); }
console.log(d.join(","));
