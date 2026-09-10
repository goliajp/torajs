// A this-reading static method's value, escaped and then called: the
// receiver is whatever the call site binds, never the class object by
// compile-time mint. §10.2.1.2 (class bodies are strict): a bare call
// binds undefined; `.call` binds its thisArg; a cast call keeps the
// member reference's base.
class C {
  static x = 3;
  static m() { return this === C; }
  static who() { return this; }
  static readX() { return this.x; }
  static both(a: number) { return [this === C, arguments.length, a]; }
  static k() { return 1; }
}
class D extends C {}

var r = C.m;
console.log("bare", C.m(), r(), D.m(), (C.m as any)(), C.m!());
console.log("call", r.call(C), r.call({}), r.call(D));
var w = C.who;
console.log("who", w(), w.call(D) === D);
var rx = C.readX;
try { console.log(rx()); } catch (e) { console.log("readX", e instanceof TypeError); }
var b = C.both;
console.log("both", b(5), b.call({}, 1, 2), (C.both as any)("x", 2, 3));
const o = { m: C.m };
console.log("objlit", o.m(), [C.m][0](), (function () { return C.m; })()());
function take(f: any) { return f(); }
console.log("arg", take(C.m));
var rd = D.m;
console.log("inherited", rd());
var rk = C.k;
console.log("this-free", rk(), rk.call({}));
console.log("identity", r === C.m, r.name, r.length);
