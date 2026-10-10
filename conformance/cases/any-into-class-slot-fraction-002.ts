// A class reached through a value (`new KK(v)` with `KK: any`) takes an
// argument for a class-annotated constructor parameter as a copy of the
// class: the copy's number fields keep fractions even when the program
// itself only writes integers into them.
class P {
  a: number;
  constructor(a: number) {
    this.a = a;
  }
}
class K {
  p: P;
  constructor(p: P) {
    this.p = p;
  }
}
const direct = new K(new P(2));
console.log(direct.p.a);
const KK: any = K;
const k = new KK(JSON.parse('{"a":1.5}'));
console.log(k.p.a);
const k2 = Reflect.construct(K, [{ a: -0.25 }]);
console.log(k2.p.a);
