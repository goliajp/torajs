// `s?.area()` where `s` may be undefined and, when present, is a class
// instance or a literal behind the interface.
interface Shape {
  area(): number;
}
class Sq {
  k = 0;
  side: number;
  constructor(s: number) {
    this.side = s;
  }
  area() {
    return this.side * this.side;
  }
}
function f(s: Shape | undefined): number | undefined {
  return s?.area();
}
console.log(f(new Sq(3)), f(undefined));
const lit: Shape = { area: () => 7 };
console.log(f(lit));
function g(s: Shape | undefined): number {
  return (s?.area() ?? 100) + 1;
}
console.log(g(new Sq(2)), g(undefined), g(lit));
