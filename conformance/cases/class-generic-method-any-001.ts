// 402-01 face 2 — generic methods dispatched on an `any`-held
// receiver: every generic `__cm_`/`__sm_` body gets an all-`any`
// mono instance ($$anywv) whose row the method registry / static
// reify strip back to the user-visible name.
class C {
  n: number = 10
  id<T>(v: T): T { return v }
  both<A, B>(a: A, b: B): A { return a }
  plus<T>(v: T, k: number): number { return this.n + k }
  static sid<T>(v: T): T { return v }
}
class D extends C {}
const tc = new C();
console.log(tc.id(1), tc.both(2, "x"), tc.plus("z", 3), C.sid(4));
const c: any = new C();
console.log(c.id(9), c.both(8, 7), c.plus(true, 5), c.n);
const d: any = new D();
console.log(d.id("inh"));
const S: any = C;
console.log(S.sid("st"));
const m = c.id;
console.log(typeof m);
console.log(c.id.length, C.sid.length);
const f2 = c.id;
console.log(f2.call(null, 42));
console.log(f2("det"));
