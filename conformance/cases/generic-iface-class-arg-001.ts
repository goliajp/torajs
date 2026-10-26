// A class instance handed to a generic function's interface parameter,
// to a constrained type parameter, inside an array of the interface and
// in a call left one argument short: the class fills the interface's
// rows (TypeScript admits it), whatever fields of its own it has, and
// the parameter binds no type variable.
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: number;
  n = 1;
  constructor(v: number) {
    this.v = v;
  }
  m(): number {
    return this.v;
  }
}
class P implements J {
  n = 10;
  m(): number {
    return 2;
  }
}
function call<T>(o: J, t: T) {
  return o.m() + o.n;
}
function gen<T extends J>(t: T) {
  return t.m() + 1;
}
function all<T>(js: J[], t: T) {
  return js.length + js[0].m();
}
function pad(o: J, t) {
  return o.n;
}
console.log(call(new S(2), 0), call(new P(), "s"), call({ m: () => 5, n: 6 }, 0));
console.log(gen(new S(3)), gen(new P()), all([new P(), new S(4)], 0), pad(new P()));
