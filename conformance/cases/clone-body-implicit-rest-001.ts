// An arrow with an unannotated rest parameter takes the element type a
// variadic annotation gives it, in a method that reads `this` too: the
// method's body is copied for calls on a receiver of another shape, and
// the copy's arrow is typed the same way.

class C {
  v = 1;
  m(): number {
    const sum: (...xs: number[]) => number = (...xs) => xs.reduce((a, b) => a + b, 0);
    return this.v + sum(1, 2, 3);
  }
}
const c = new C();
console.log(c.m());
const m: any = c.m;
console.log(m.call({ v: 42 }));
const o: any = { v: 7, m: c.m };
console.log(o.m());
