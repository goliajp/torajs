// An object literal of an interface whose method returns an any value,
// an argument of a call whose argument list keeps a dynamic spread: the
// literal lands on its own position before the spread, and on any
// later one after it (§13.3.8.1).
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
function after(a: number, o: J) {
  return a + o.m();
}
function before(o: J, a: number) {
  return o.m() * a;
}
function g(): number {
  return 2.5;
}
const t = JSON.parse("[1]") as [number];
console.log(after(...t, { m: g, n: 1 }), after(1, new S(raw)), g());
console.log(before({ m: g, n: 1 }, ...t), before(new S(raw), 2));
