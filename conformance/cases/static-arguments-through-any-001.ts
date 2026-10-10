// A static method that reads `arguments`, called through an `any`
// value of its class: the count is the call's argument count, whether
// or not the arguments fit the declared parameter types.
class K1 {
  static n(a: number) {
    return arguments.length;
  }
}
const k1: any = K1;
console.log(k1.n(1, 2, 3, 4));
class K2 {
  static n(a: number) {
    return arguments.length;
  }
}
const k2: any = K2;
console.log(K2.n(1), k2.n(1, 2, 3, 4), k2.n());
class K4 {
  static n(a: number) {
    return arguments.length;
  }
}
const k4: any = K4;
console.log(K4.n(1), K4.n(1, 2), k4.n(1, 2, 3, 4));
class K {
  static tag = "K";
  static m(a: number) {
    return this.tag + ":" + arguments.length + ":" + a;
  }
}
const o = { tag: "o" };
console.log(K.m(1), K.m.call(o, 2, 3), K.m.apply(o, [4, 5, 6]));
const kk: any = K;
console.log(kk.m(7, 8), kk.m("x"));
class J {
  static n(a: boolean, b: string) {
    return arguments.length + ":" + a + ":" + b;
  }
}
const jj: any = J;
console.log(J.n(true, "s"), jj.n(1, 2, 3), jj.n());
