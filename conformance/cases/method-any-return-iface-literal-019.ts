// Object literals of an interface whose method returns an any value
// whose member holds a function the source does not lead back to a
// declaration: a call's result, a method read off another object, a
// binding assigned twice; and a function with no return annotation
// returning a binding annotated with the interface.
const raw: any = JSON.parse('{"x":1.5,"s":"ab"}');
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
class T implements J {
  v: any;
  n = 2;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.s;
  }
}
function call(o: J) {
  return o.m() + o.n;
}
function mkf(k: number): () => number {
  return () => k + 0.5;
}
const h = {
  f(): number {
    return 6.75;
  },
};
let r: () => number = mkf(1);
r = mkf(2);
const o1: J = { m: mkf(3), n: 1 };
const o2: J = { m: h.f, n: 2 };
const o3: J = { m: r, n: 3 };
function mk() {
  const o: J = { m: r, n: 5 };
  return o;
}
console.log(call(o1), call(o2), call(o3), call(mk()));
console.log(call(new S(raw)), call(new T(raw)), r(), h.f());
