// §10.2.2 step 13.c at a capturing-lane `super(…)` site: the site is
// where the PARENT's [[Construct]] step 13 runs, and only a DERIVED
// parent turns a non-object, non-undefined answer into a TypeError — a
// base parent's primitive is ignored and `this` stands. The lane used
// to treat every parent as base: a nested subclass of a derived real
// class whose ctor returns a primitive constructed silently.
class A { v = 1; }
class P extends A { constructor() { super(); return 5 as any; } }
class Base { constructor() { return 7 as any; } }
class S extends Base { w = 3; constructor() { super(); } }
class T extends S { constructor() { super(); } }
class U extends T { constructor() { super(); return 9 as any; } }
function attempt(label: string, f: () => any): void { try { console.log(label, f()); } catch (e: any) { console.log(label, "TypeError", e instanceof TypeError); } }
attempt("top derived", () => new P() instanceof P);
function mk(k: number): any {
  class R extends P { k(): number { return k; } }
  return new R().k();
}
attempt("nested over derived real parent", () => mk(1));
function mk2(k: number): any {
  class R2 extends Base { k(): number { return k; } }
  return new R2().k();
}
attempt("nested over base real parent", () => mk2(9));
function mk3(k: number): any {
  class M extends A { constructor() { super(); return k as any; } }
  class N extends M { k(): number { return k; } }
  return new N().k();
}
attempt("nested over derived sibling", () => mk3(4));
function mk4(k: number): any {
  class M extends A { constructor() { super(); return { k: () => k } as any; } }
  class N extends M { }
  return (new N() as any).k();
}
attempt("nested over sibling returning object", () => mk4(6));
function mk5(k: number): any {
  class N extends U { k(): number { return k; } }
  return new N().k();
}
attempt("nested over derived chain", () => mk5(2));
function mk6(k: number): any {
  class N extends S { k(): number { return k; } }
  return new N().k();
}
attempt("nested over derived parent returning undefined", () => mk6(8));
