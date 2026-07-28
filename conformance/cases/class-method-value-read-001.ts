// S2.34 — method VALUE reads off a class instance: toplevel and
// inside a method body (`this.m`), plain and generator, then invoked
// through `.call` with an explicit receiver.
class C {
  m() { return 5; }
  *g() { yield 3; }
  grabM() { return this.m; }
  grabG() { return this.g; }
}
var inst = new C();
var f1 = inst.m;
console.log(typeof f1, f1.call(inst));
var f2 = new C().grabM();
console.log(typeof f2, f2.call(inst));
var f3 = new C().grabG();
var it = f3.call(new C());
console.log(typeof f3, it.next().value, it.next().done);
