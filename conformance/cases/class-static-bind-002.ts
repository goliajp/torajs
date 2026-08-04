// RFC 20260804-fn-this-channel knife 5 — .bind on a static method.
// A static's mono has no receiver channel (its `this` binds to the
// class name), so a rebind can only be served by the __smany_ twin.
// call/apply already retargeted there; bind now does too, which means
// the retarget has to happen before the swallow test — the twin
// carries one param more than the mono, and bind spends a param slot
// per partial.
class C {
  v = 7;
  static g(n: number) {
    return (this as any).v + n;
  }
  static free() {
    return 99;
  }
}
console.log(C.g.bind(new C())(1));
console.log(C.g.bind(new C(), 5)());
console.log(C.g.bind({ v: 100 })(2));

// a this-free static needs no receiver and keeps the historic drop
console.log(C.free.bind(null)());

// no brand, no receiver: reading v off undefined is a TypeError
try {
  C.g.bind(undefined)(1);
  console.log("no throw");
} catch (e) {
  console.log((e as any).constructor.name);
}

// the bound static is a value that survives being passed around
const bound = C.g.bind(new C());
function apply1(cb: any, n: number) {
  return cb(n);
}
console.log(apply1(bound, 3));
