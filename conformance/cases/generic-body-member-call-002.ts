// A method call inside a closure of a generic function's body, whose
// name only one class declares: each specialization's copy of the
// closure calls the receiver's own member, an object literal's as well
// as the class instance's.
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
  const f = () => o.m();
  return f();
}
const s: J = new S(JSON.parse('{"x":1.5}'));
console.log(call(s, 0) + 1, call({ m: () => 3 }, "a") + 1);
