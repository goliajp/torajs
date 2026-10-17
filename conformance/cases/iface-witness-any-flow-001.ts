// A class that fills an object type's method rows without declaring it
// reaches a slot of that type through `any`: the value keeps its
// identity and the call runs the class's own method.
type T = { m: () => number };
class K4 {
  x = 1;
  m() {
    return 9 + this.x;
  }
}
class K5 {
  y = 2;
  m() {
    return 20 + this.y;
  }
}
function mk(i: number): any {
  return i % 2 === 0 ? new K4() : new K5();
}
const p: T = mk(0);
console.log(p.m());
let q: T | undefined;
q = mk(1);
console.log(q.m());
class Holder {
  t: T;
  constructor(t: T) {
    this.t = t;
  }
}
const h = new Holder(mk(0));
h.t = mk(1);
console.log(h.t.m());
function back(v: any): T {
  return v;
}
const r = back(mk(0));
console.log(r.m());
const k = new K4();
const s: T = k as any;
k.x = 100;
console.log(s.m(), s === (k as any));
