// A class whose ctor parameter is un-annotated gets a genericized
// factory (`__new_C$$…` monos), and its instances wear per-mono tags
// with the class's name — exactly like an explicit generic's. The
// instanceof answer set listed only the template's constant tag, so
// `new C(1) instanceof C` answered false, and the private brand check
// (the same membership test) refused the class's own `this.#x = v`
// once the base ctor had used `this`. Self, ancestor, any receiver,
// nested chains and the private field all answer like bun.
class NB { constructor(seal) { this.s = seal; } }
class C1 extends NB { #val; constructor(seal) { super(seal); this.#val = 42; } val(): any { return this.#val; } static has(o: any): boolean { return #val in o; } }
class C2 { constructor(seal) { } }
class D { }
class E<T> { v: T; constructor(x: T) { this.v = x; } }
class F extends C2 { constructor() { super(0); } }
const c = new C1(false), c2 = new C2(1), d = new D(), e = new E("x"), f = new F();
const ca: any = c, fa: any = f;
console.log(c instanceof C1, c instanceof NB, c instanceof Object, c instanceof D, c2 instanceof C2, d instanceof D, e instanceof E, f instanceof F, f instanceof C2);
console.log(ca instanceof C1, ca instanceof NB, fa instanceof C2, ca instanceof C2, C1.has(c), C1.has(d), c.val());
const xs: any[] = [new C1(1), new C1("s"), new C2(2), d];
console.log(xs.map((o: any) => [o instanceof C1, o instanceof NB, o instanceof C2].join("")).join(" "));
function mk(k: number): any { class L { constructor(v) { this.v = v; } } const l: any = new L(k); return [l instanceof L, l.v]; }
console.log(mk(3), mk(4));
