// An any value handed to a generic function's object parameter, directly
// and through the function's value held as any: the parameter holds the
// value as it is (§10.2.1.3), a class instance, an object literal made
// in the any world or one of another layout, and a member read on it
// reads that value's member (§13.3.2.1).
interface J {
  m(): number;
}
class S implements J {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function call<T>(o: J, t: T): number {
  return o.m();
}
const raw: any = JSON.parse('{"x":1.5}');
const a: any = new S(raw);
const b: any = { m: () => 3 };
const c: any = { k: 1, m: () => 7 };
console.log(call(a, 0) + 1, call(b, "s") + 1, call(c, 0) + 1);
const s: J = new S(raw);
console.log(call(s, 0) + 1);
const f: any = call;
console.log(f({ m: () => 3 }, 0) + 1, f(new S(raw), 0) + 1, f(s, 0) + 1, f(c, "x") + 1);
