// rotation 240 — typed setter params keep their lanes under the
// generalized arg_conv route: the F2-fix i64→f64 widen face must not
// regress, and an Any rhs into a typed param unboxes instead of
// handing NaN-box bits to a typed body.
class C {
  f = 0;
  s = "";
  set num(v: number) {
    this.f = v;
  }
  set txt(v: string) {
    this.s = v;
  }
  report() {
    return [this.f, this.s];
  }
}
const c = new C();
c.num = 3;
console.log(c.report());
c.num = 2.75;
console.log(c.report());
const a: any = 41;
c.num = a;
console.log(c.report());
const b: any = "boxed";
c.txt = b;
console.log(c.report());
c.txt = "plain";
console.log(c.report());
