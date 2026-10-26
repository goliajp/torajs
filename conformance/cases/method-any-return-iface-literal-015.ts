// The function an object literal holds in a member whose interface
// method returns an any value is the same function object the program
// declared: identity, name and a Map key survive the member. The member
// is read off a receiver typed `any`, as a typed receiver's value read
// keeps the interface's declared return.
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
function g(): number {
  return 4;
}
const o: J = { m: g, n: 1 };
const d: any = o;
const seen = new Map<Function, string>([[g, "g"]]);
function call(o: J) {
  return o.m();
}
console.log(d.m === g, d.m.name, seen.get(d.m), call(o), o.m(), call(new S()));
