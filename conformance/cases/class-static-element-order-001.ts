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

// 563-03 other half — a COMPUTED static member sits at its
// declaration position too. Its key exists only at the class-decl
// position, long after the prologue's registration walk defined the
// plain members, and an own entry can only be appended — so tr
// answered ["a","t","s1"] for the first one below.
const s1 = "k1";
const s2 = "k2";

class H { static a() {} static [s1]() {} static t() {} }
console.log(ks(H));

class I { static p() {} static get [s1]() { return 1; } static q() {} }
console.log(ks(I));

// Two computed members, plain ones between and after.
class J {
  static a() {} static [s1]() {} static b() {} static [s2]() {} static c() {}
}
console.log(ks(J));

// Computed first — nothing to move.
class K { static [s1]() {} static a() {} }
console.log(ks(K));

// Computed last — nothing to move either.
class L { static a() {} static [s1]() {} }
console.log(ks(L));

// Static fields still materialize after every method (step 29), so
// the methods sort among themselves and the fields follow.
class M { static x = 1; static [s1]() {} static y = 2; static m() {} }
console.log(ks(M));

// An accessor pair under a computed key, with plain members around.
class N {
  static get t() { return 1; }
  static [s1]() {}
  static set t(v: number) {}
  static u() {}
}
console.log(ks(N));

// A computed static FIELD is created in the initializer pass, so it
// lands after the plain methods no matter where it is declared.
class O { static a() {} static [s1] = 5; static b() {} }
console.log(ks(O));

// The moved entries keep their values and attributes.
console.log(H.a === H.a, typeof H.t, (H as any)[s1] === (H as any)[s1]);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(N, "t")!.enumerable));
console.log(N.t, (O as any)[s1], M.x, M.y);
