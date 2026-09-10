// Under a runtime super base (the program relinks prototypes), a super write goes through the
// §9.1.9 kernel: the base's LIVE chain decides whether a setter runs, a null base is the
// TypeError after the value has been evaluated, and the store still lands on the receiver.
// super.m?.() / super[k]?.() are §13.3.9.1 optional calls with the call site's this.
var count = 0;
class C { static m() { super.x = count += 1; } static n() { super[0] = count += 1; } static d() { delete super.x; } }
Object.setPrototypeOf(C, null);
try { C.m(); console.log("no throw"); } catch (e) { console.log((e as any).constructor.name, count); }
try { C.n(); console.log("no throw"); } catch (e) { console.log((e as any).constructor.name, count); }
try { C.d(); console.log("no throw"); } catch (e) { console.log((e as any).constructor.name); }
var relinked = 0;
class B { set s(v: number) { relinked = v * 2; } }
class D extends B { m() { super.s = 4; return relinked; } w() { super["t"] = 9; return (this as any).t; } }
// the relinked literal rides the any lane (a dynobj level the set walk can read a setter off)
var re: any = { set s(v: number) { relinked = v * 3; } };
Object.setPrototypeOf(D.prototype, re);
const d = new D();
console.log(d.m(), d.w(), Object.getOwnPropertyDescriptor(d, "t")!.value, Object.getOwnPropertyDescriptor(d, "s") === undefined);
let called = false;
let context: any;
class Base { method() { called = true; context = this; return "r"; } }
class Foo extends Base { method() { return super.method?.(); } q() { return super["nope"]?.(); } k() { return super["method"]?.(); } }
const foo = new Foo();
console.log(foo.method(), foo === context, called, foo.q(), foo.k());
class P { static v = 1; static get acc() { return 7; } }
class Q extends P { static r() { return [super.v, super.acc, super.missing?.()]; } }
console.log(Q.r().join(","));
