// An object type literal with the members of an interface whose method
// returns an any value, written in another order (`h4`): it checks as
// a type of its own and keeps the return it spells, so an object
// literal handed to it is still called through it. One written in the
// interface's order (`h3`) is retyped with the interface.
const raw: any = JSON.parse('{"x":1.5}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function h3(o: { m(): number; n: number }) {
  return o.m();
}
function h4(o: { n: number; m(): number }) {
  return o.m();
}
function hj(o: J) {
  return o.m();
}
console.log(h3({ m: () => 4, n: 3 }));
console.log(hj({ m: () => 4, n: 3 }));
const l: { m(): number; n: number } = { m: () => 6, n: 1 };
console.log(h3(l), new S(raw).m());
console.log(h4({ n: 1, m: () => 2 }));
