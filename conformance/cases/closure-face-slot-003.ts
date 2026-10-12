// A function value of another face handed back by a closure declared
// to return the slot's type, called at once or through the binding
// that holds it; and the same through a method's return.
const mk2 = (k: number) => (y?: number) => (y === undefined ? 5 : y) + k;
const mks = (k: number) => (y?: number): any => "s" + k;
type T = (y?: number) => number;

const r = (): T => mk2(1);
console.log(r()(), r()(4));
const rr = function (): T {
  return mk2(2);
};
console.log(rr()());
const rs = (): T => mks(5);
const v = rs()();
console.log(v, typeof v);
const h = r();
console.log(h(), h(1));

class C {
  m(): T {
    return mk2(3);
  }
  ms(): T {
    return mks(6);
  }
  static s(): T {
    return mk2(4);
  }
}
const c = new C();
console.log(c.m()(), c.m()(1), C.s()());
const w = c.ms()();
console.log(w, typeof w);
const hm = c.m();
console.log(hm(), hm(2));
