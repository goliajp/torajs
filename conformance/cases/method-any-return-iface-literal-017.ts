// A function reading `this`, held by object literals in a member whose
// interface method returns an any value, called through the literal
// and through the class. The two members are compared off receivers
// typed `any`.
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any = JSON.parse('{"x":1.5}');
  n = 1;
  m(): number {
    return this.v.x;
  }
}
function g(this: J): number {
  return this.n * 10;
}
const o: J = { m: g, n: 7 };
const o2: J = { m: g, n: 8 };
function call(o: J) {
  return o.m();
}
const a: any = o;
const b: any = o2;
console.log(call(o), call(o2), a.m === b.m, call(new S()));
