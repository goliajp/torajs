// 399-01 — a class member with NO return annotation whose every
// value-return is a marked fn-expr gets its annotation seeded `any`,
// so the returned function takes its `this` at the CALL SITE instead
// of inheriting the method's receiver.

// instance method, bare annotation
class M {
  v = 1;
  bare() {
    return function () {
      return (this as any).v;
    };
  }
}
console.log(new M().bare().call({ v: 43 }));

// static method, bare annotation
class N {
  static s() {
    return function () {
      return (this as any).k;
    };
  }
}
console.log(N.s().call({ k: 21 }));

// a this-free returned fn-expr keeps its concrete type (no seeding)
class P {
  bare3() {
    return function (x: any) {
      return x + 1;
    };
  }
}
console.log(new P().bare3()(41));

// a mixed-return body keeps today's behavior (no seeding)
class Q {
  m(c: boolean) {
    if (c) {
      return function () {
        return (this as any).v;
      };
    }
    return 5;
  }
}
console.log(new Q().m(false));

// the returned value survives a store into an any expando
const holder: any = { v: 9 };
class R {
  bare() {
    return function () {
      return (this as any).v;
    };
  }
}
holder.f = new R().bare();
console.log(holder.f());
