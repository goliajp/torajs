// A function value of another face handed to a method called through a
// member expression: an object literal's method, a class's method and
// its overrides call the value by its own convention.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }
function mkA(k: number): (y?: number) => any { return (y?: number) => (y === undefined ? 5 : y) * k }

const obj = {
  base: 100,
  m(f: (y?: number) => number) { return f() + f(3) },
  t(f: (y?: number) => number) { return this.base + f() },
  w: (a: number, f: (y: number) => number) => f(a) + 1,
};
console.log(obj.m(mk2(1)), obj.m(mkA(2)), obj.m((y?: number) => 1));
console.log(obj.t(mk2(1)), obj.w(4, mkA(3)), obj.w(4, (y: number) => y));
function inner() {
  const o2 = { m(f: (y?: number) => number) { return f(1) * 2 } };
  return o2.m(mkA(5)) + o2.m(mk2(0));
}
function viaGlobal() { return obj.m(mk2(10)); }
console.log(inner(), viaGlobal());

class K {
  run(f: (y?: number) => number) { return f() + f(3) }
  static s(f: (y?: number) => number) { return f() * 2 }
}
const k = new K();
console.log(k.run(mk2(1)), K.s(mk2(2)), k.run((y?: number) => 2));

class Base {
  m(f: (y?: number) => number) { return f() + f(3) }
}
class Sub extends Base { m(f: (y?: number) => number) { return f(1) * 10 } }
class Plain extends Base {}
const bs: Base[] = [new Base(), new Sub(), new Plain()];
console.log(bs.map((x) => x.m(mk2(2))).join(","));
function viaParam(x: Base) { return x.m(mkA(3)); }
console.log(viaParam(new Sub()), viaParam(new Plain()), new Plain().m(mk2(4)));
