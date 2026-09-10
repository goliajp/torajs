// SuperProperty in the static context beyond static method bodies (§15.7.14 step 31): a
// static block and a static field initializer run with the class as this and as the home
// object, so super.x there reads off the parent class object (or %Function.prototype% for a
// base class) and super.x = v writes onto the class object.
class Parent { static test262 = "test262"; }
var value: any;
class C extends Parent { static { value = super.test262; } }
console.log(value);
class P2 { static s() { return "ps"; } static q = 4; }
class D extends P2 { static { value = super.s() + super.q; } static early = super.q * 10; }
console.log(value, D.early);
class L { static f = () => { super.sp = "s"; }; static g = () => super.hasOwnProperty === Function.prototype.hasOwnProperty; }
L.f(); console.log((L as any).sp, L.g());
class K { static m() { return super.toString === Function.prototype.toString; } static w() { super.q = 3; return (K as any).q; } }
console.log(K.m(), K.w());
