// A class instance held by one interface and then handed to another:
// the second interface reads and writes the class's own field, and a
// literal of the first interface's shape passes through the same way.
interface A {
  x: number;
  m(): number;
}
interface B {
  x: number;
}
class C {
  k = 0;
  x = 5;
  m() {
    return this.x + 1;
  }
}
const a: A = new C();
const b: B = a;
console.log(b.x, a.m());
b.x = 7;
console.log(a.x, a.m());
const lit: A = { x: 2, m: () => 9 };
const b2: B = lit;
console.log(b2.x, lit.m());
function readB(v: B): number {
  return v.x * 10;
}
console.log(readB(a), readB(lit), readB(new C()));
