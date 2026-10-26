// A capturing closure an object literal holds in a member whose
// interface method returns an any value, called through the literal and
// directly.
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
function make(base: number) {
  const g = () => base * 2;
  const o: J = { m: g, n: 1 };
  return [o, g] as const;
}
const [o, g] = make(21);
console.log(o.m(), g(), o.m === g, new S().m());
