// A `super(…)` whose parent is a runtime VALUE (`class R extends f()`)
// reaches the parent's constructor through the value kernel, and that
// call is where the PARENT's §10.2.2 step 13 runs. The site's pick used
// to treat every value parent as a base class: a DERIVED parent's ctor
// answering a primitive constructed silently. The kernel now reads the
// parent's registered [[ConstructorKind]] — a base parent's primitive is
// still ignored, an object answer still adopted, a plain function parent
// is base.
class A { v = 1; }
class P extends A { constructor() { super(); return 5 as any; } }
class B { constructor() { return 7 as any; } }
class O extends A { constructor() { super(); return { who: "o" } as any; } }
class BI extends Array<number> { constructor() { super(); return 3 as any; } }
function pick(c: any): any { return c; }
function attempt(label: string, f: () => any): void { try { console.log(label, f()); } catch (e: any) { console.log(label, "TypeError", e instanceof TypeError); } }
function mk(k: number): any { class R extends pick(P) { k(): number { return k; } } return new R().k(); }
function mk2(k: number): any { class R extends pick(B) { k(): number { return k; } } return new R().k(); }
function mk3(k: number): any { class R extends pick(O) { k(): number { return k; } } return (new R() as any).who + k; }
function mk4(k: number): any { class R extends pick(BI) { k(): number { return k; } } return new R().k(); }
function fnParent(this: any) { return 9; }
function mk5(k: number): any { class R extends pick(fnParent) { k(): number { return k; } } return new R().k(); }
attempt("value parent derived, primitive", () => mk(1));
attempt("value parent base, primitive", () => mk2(2));
attempt("value parent derived, object", () => mk3(3));
attempt("value parent extends builtin, primitive", () => mk4(4));
attempt("value parent plain function", () => mk5(5));
attempt("direct builtin-extending, primitive", () => new BI() instanceof BI);
class T extends pick(P) { constructor() { super(); } }
attempt("top-level value parent derived", () => new T() instanceof T);
class U extends pick(B) { constructor() { super(); } }
attempt("top-level value parent base", () => new U() instanceof U);
