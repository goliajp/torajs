// An object literal of an interface whose method returns an any value,
// an argument of a generic function: the specialization calls the
// member through the interface, as the generic body is written, not
// the method of the one class that declares a method of that name,
// and the literal's member stores are seen on the generic call path.
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
function call<T>(o: J, t: T) {
  return o.m();
}
function plain(o: J) {
  return o.m();
}
function g(): number {
  return 2.5;
}
console.log(call({ m: g, n: 1 }, 0), call({ m: () => 3.5, n: 1 }, "s"), plain(new S(raw)), g());
