// A class instance is a value of every interface whose members it
// declares, with no `implements` clause needed; a call through the
// interface reaches the instance's own method, an override included.
interface Shape {
  area(): number;
}
class Sq {
  s: number;
  constructor(s: number) {
    this.s = s;
  }
  area() {
    return this.s * this.s;
  }
}
class Base {
  name: string;
  constructor(n: string) {
    this.name = n;
  }
  area(): number {
    return 0.5;
  }
}
class Big extends Base {
  w: number;
  constructor(w: number) {
    super("big");
    this.w = w;
  }
  area(): number {
    return this.w * 1.25;
  }
}
const sh: Shape = new Sq(3);
console.log(sh.area());
const b: Shape = new Big(4);
console.log(b.area());
const base: Base = new Big(2);
const viaBase: Shape = base;
console.log(viaBase.area(), new Base("x").area());
let total = 0;
for (let i = 0; i < 1000; i++) {
  const s: Shape = i % 2 === 0 ? new Sq(i % 7) : new Big(i % 5);
  total += s.area();
}
console.log(total);
