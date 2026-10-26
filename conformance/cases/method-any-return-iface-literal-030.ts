// An object literal of an interface built in a generic function's body,
// holding a function declared to return a number, beside a class whose
// method returns an any value: the literal is typed only in the
// specializations, and the interface keeps the return its literal's
// function answers.
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
function g(): number {
  return 2;
}
function mk<T>(t: T): J {
  const o = { m: g, n: 1 };
  return o;
}
function call(j: J): number {
  return j.m() + 1;
}
const raw: any = JSON.parse('{"x":4}');
console.log(call(mk(0)), call(mk("s")), call(new S(raw)), g());
