// The function an object literal holds in a member whose interface
// method returns an any value is the same function object the program
// declared: identity, name and a Map key survive the member, read off a
// receiver typed `any` and off one typed with the interface (§7.2.10,
// §13.3.6.1), and the class row still returns its any value.
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
console.log(o.m === g, o.m.name, seen.get(o.m), seen.has(o.m), typeof o.m, call(new S()));
