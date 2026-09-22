// §13.15.2 — an assignment through a class setter evaluates to the rhs
// value. A setter that does not store the value must not leave a kept
// result pointing at a released temporary.
class C {
  _p = "";
  set p(x: string) {}
  get p() {
    return this._p;
  }
}
class D {
  _q = "";
  set q(x: string) {
    this._q = x;
  }
  get q() {
    return this._q;
  }
}
const c = new C();
const d = new D();
const rs: string[] = [];
for (let i = 0; i < 3; i++) {
  const r = (c.p = "k" + i);
  rs.push(r);
}
console.log(rs.join(","), JSON.stringify(c.p));
const qs: string[] = [];
for (let i = 0; i < 3; i++) {
  const a = "a" + i;
  d.q = a;
  const r = (d.q = "b" + i);
  c.p = "z" + i;
  qs.push(r, a, d.q);
}
console.log(qs.join(","));
