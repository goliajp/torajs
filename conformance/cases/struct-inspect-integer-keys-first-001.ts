// §10.1.11.1 — a class instance's inspect block lists integer-index
// field names first, ascending, then the rest in declaration order —
// the same order Object.keys and JSON.stringify already walk.
class C { b = 1; 10 = 2; a = 3; 2 = "two"; }
var c: any = new C();
console.log(c);
console.log(Object.keys(c).join(","), JSON.stringify(c));
class D { z = 0; get 5() { return 5; } y = "y"; }
console.log(new D());
class E { 1 = 1; 0 = 0; }
var e: any = new E();
e.extra = true;
console.log(e);
class F { get 7() { return 7; } z = 0; 3 = "three"; set q(v: any) {} }
console.log(new F());
