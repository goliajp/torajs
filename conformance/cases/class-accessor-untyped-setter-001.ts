// rotation 240 — an untyped accessor setter (`set p(v)` — Any param)
// behind the typed-receiver direct-call lane: the argument used to
// bypass arg_conv entirely, so an i64 rhs arrived as raw bits and the
// bare-field store held a garbage NaN-box (p25g SIGSEGV / silent
// no-output). Every rhs lane crosses here now.
class C {
  s;
  get p() {
    return 17;
  }
  set p(v) {
    this.s = v;
  }
  run() {
    this.p = 5;
    return this.s;
  }
}
const c = new C();
console.log(c.run());
c.p = 2.5;
console.log(c.s);
c.p = "text";
console.log(c.s);
c.p = true;
console.log(c.s);
c.p = null;
console.log(c.s);
console.log(c.p);
