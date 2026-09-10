// A class whose parent is a plain FUNCTION (the ES5-value-parent lane): super property
// sites in its static blocks and static field initializers read off the parent function
// object (§13.3.7, home object = the class) and write onto the class (§9.1.9).
function Parent() {}
(Parent as any).test262 = "test262";
(Parent as any).n = 5;
var value: any;
class C extends Parent { static { value = super.test262; super.w = 9; } static f = super.n * 10; }
console.log(value, (C as any).w, (C as any).f, Object.getOwnPropertyNames(C).includes("w"));
(Parent as any).prototype.greet = function (this: any) { return "hi " + this.n; };
class D extends Parent {
  n = "d";
  m() { return super.greet(); }
  w() { super.tag = "t"; return (this as any).tag; }
  static s() { return super.n === undefined ? "no n" : "n"; }
}
const d: any = new D();
console.log(d.m(), d.w(), D.s());
