// An object literal of the type of an interface whose method returns an
// any value holds a function in that member by other spellings than a
// closure written in place: a const binding of a closure, a function
// declaration by shorthand, and a conditional of closures,
// each followed to its declaration, which returns its value as it is
// with the interface.
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
function call(o: J): number {
  return o.m() + o.n;
}
function h3(o: { m(): number; n: number }) {
  const a = o.m();
  return a * 2;
}
const f = () => 4;
const o1: J = { m: f, n: 1 };
console.log(call(o1), call(new S(raw)), h3(o1));
function g(): number {
  return 5;
}
const mm = g;
const o2: J = { m: mm, n: 2 };
console.log(call(o2), g(), h3(o2));
function outer() {
  function m() {
    return 5;
  }
  return h3({ m, n: 1 }) + call({ m, n: 1 });
}
console.log(outer());
const flip = raw.x > 1;
const o3: J = { m: flip ? () => 6 : () => 7, n: 3 };
console.log(call(o3), h3({ m: flip ? () => 6 : () => 7, n: 3 }));
const k = () => 9;
const q: J = { m: () => 10, n: 2 };
console.log(call({ m: k, n: 1 }), call(q), k(), h3({ m: k, n: 0 }));
