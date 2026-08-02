// §15.4.1 accessor-arity legal faces: a zero-param getter and a
// one-param setter (class + object literal) must keep working after
// the arity early errors landed (get with params / set with zero,
// two, or a rest param are now parse-time SyntaxErrors).
class C {
  _v: any = 1;
  get x(): any {
    return this._v;
  }
  set x(v: any) {
    this._v = v + 1;
  }
}
const c: any = new C();
console.log(c.x);
c.x = 5;
console.log(c.x);

const o: any = {
  _w: 2,
  get y() {
    return this._w;
  },
  set y(v: any) {
    this._w = v * 2;
  },
};
console.log(o.y);
o.y = 3;
console.log(o.y);
