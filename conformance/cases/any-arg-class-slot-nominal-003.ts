// a parameter typed by a class admits, through `any`, an instance of
// any class below it: a subclass of a subclass, and a class
// expression that extends it; each runs its own methods
class P {
  x = 1;
  m() {
    return "P" + this.x;
  }
  k() {
    return "k" + this.x;
  }
}
class R extends P {
  y = 2;
  m() {
    return "R" + this.y;
  }
}
class S extends R {
  z = 3;
  m() {
    return "S" + this.z;
  }
}
// keeps `m` from R
class T extends R {}
const C = class extends P {
  m() {
    return "C" + this.x;
  }
};
const D = class extends S {};
// same fields as P, not below it
class Q {
  x = 1;
  m() {
    return "Q" + this.x;
  }
  k() {
    return "q" + this.x;
  }
}
function f(p: P) {
  return p.m() + ":" + p.k() + ":" + (p instanceof P) + ":" + (p instanceof R);
}
const g: any = f;
const xs: any[] = [new P(), new R(), new S(), new T(), new C(), new D(), new Q()];
for (const x of xs) console.log(g(x));
console.log(f(new S()), f(new C()), f(new D()));

function all(ps: P[]) {
  return ps.map((p) => p.m()).join(",");
}
const h: any = all;
console.log(h([new S(), new C(), new T()]));
console.log(h([new P(), new Q()]));
