// 563-03 — §15.7.14 defines every STATIC class element in the same
// ONE ordered pass the instance side gets (562-07), so a static
// accessor declared before a static method is an own key of the
// class object before it. tr ran two registration passes — every
// static method, then every static accessor pair SORTED BY NAME —
// so `class { static get t() {} static u() {} }` answered ["u","t"].
//
// `length` / `name` / `prototype` are filtered out throughout: tr
// seeds them in a different order than JSC does (562-01), which is a
// separate gap and would mask this one.
function ks(o: any): string {
  const skip = ["length", "name", "prototype"];
  return JSON.stringify(
    Object.getOwnPropertyNames(o).filter((n: string) => skip.indexOf(n) < 0),
  );
}

class A { static get t() { return 1; } static u() {} }
console.log(ks(A));

// The other way round — this one the sort happened to answer right.
class B { static u() {} static get t() { return 1; } }
console.log(ks(B));

// Two accessors before a method: their relative order is declaration
// order, not alphabetical.
class C {
  static get z() { return 1; }
  static get a() { return 2; }
  static m() {}
}
console.log(ks(C));

// A getter and its setter are ONE own key, at the first face.
class D {
  static get p() { return 1; }
  static q() {}
  static set p(v: number) {}
  static r() {}
}
console.log(ks(D));

// Static fields materialize in the initializer pass, after every
// method and accessor (§15.7.14 step 29).
class E { static x = 1; static get g() { return 2; } static m() {} }
console.log(ks(E));

// The instance side of the same class is unaffected.
class F {
  static get s() { return 1; }
  static m() {}
  get g() { return 2; }
  n() {}
}
console.log(ks(F));
console.log(JSON.stringify(Object.getOwnPropertyNames(F.prototype)));

// A subclass's static own keys are its own.
class G extends A { static get w() { return 1; } static v() {} }
console.log(ks(G));

// The descriptors survive the reordering.
const d = Object.getOwnPropertyDescriptor(A, "t")!;
console.log(typeof d.get, d.set, d.enumerable, d.configurable);
console.log(A.t, D.p, E.g, G.w);
