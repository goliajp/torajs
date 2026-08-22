// §13.3.6 boundaries of `super[k](…)`: static context, a missing
// method, an accessor base, key evaluation, shadowing, and a getter
// that throws.
class S1 { static sm() { return "S1.sm:" + this.name; } }
class S2 extends S1 { static go() { return super["sm"](); } }
console.log(S2.go());

class T1 {}
class T2 extends T1 { go() { return super["nope"](); } }
try { new T2().go(); } catch (e: any) { console.log(e.constructor.name); }

// The getter on the base runs against `this`, not against the base.
class G1 { get f() { const self: any = this; return function () { return self.tag; }; } }
class G2 extends G1 { tag = "g2"; go() { return super["f"](); } }
console.log(new G2().go());

// The key expression is evaluated exactly once.
let n = 0;
class K1 { k0() { return "k0"; } }
class K2 extends K1 { go() { return super["k" + (n++)](); } }
console.log(new K2().go(), n);

// The lexical home decides the base — a subclass override does not.
class Z1 { z() { return "Z1.z"; } }
class Z2 extends Z1 { go() { return super["z"](); } }
class Z3 extends Z2 { z() { return "Z3.z"; } }
console.log(new Z3().go());

class E1 { get bad(): any { throw new RangeError("boom"); } }
class E2 extends E1 { go() { return super["bad"](); } }
try { new E2().go(); } catch (e: any) { console.log(e.constructor.name, e.message); }
