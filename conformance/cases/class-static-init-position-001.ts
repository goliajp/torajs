// 420-02 — a class's static field initializers and static blocks run
// where the class is written (ES §15.7.14), not before the program's
// first statement. They used to be prepended to the top of the module,
// which put them ahead of every top-level `var` initializer: a static
// init calling a named function read that function's globals before
// they existed. For a scalar the write was silently overwritten by the
// declaration that ran later; for a heap global it was a SEGV.
var log: string[] = [];
function side(t: string): string { log.push(t); return t }
var n = 0;
function bump(): number { n = n + 1; return n }

console.log("A");
class C {
  static s = side("s1");
  static k = bump();
  static { log.push("block"); }
}
console.log("B", C.s, C.k, n, log.join(","));

// Same, one function level down: the class is written inside a
// function, so its definition-time work belongs to each call.
function make(): string {
  log.push("enter");
  class D { static d = side("d"); }
  return D.d;
}
console.log(make(), make());
console.log(log.join(","));

// A class with no definition-time work at all still lifts cleanly.
function plain(): number {
  class E { m(): number { return 4 } }
  return new E().m();
}
console.log(plain());
