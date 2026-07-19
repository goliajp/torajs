// RFC 20260717-fnexpr-this-channel knife 2W cut 2 — one fn-expr
// binding used as an accessor face AND called directly by its bare
// name: face invokes bind `this` to the receiver, bare-name calls
// get strict-mode `this === undefined`.
const g = function () {
  if (this === undefined) {
    return "bare";
  }
  return "recv:" + this._x;
};
const o1: any = { _x: 7 };
const o2: any = { _x: 8 };
o1.__defineGetter__("v", g);
Object.defineProperty(o2, "v", { get: g });
console.log(g());
console.log(o1.v);
console.log(o2.v);
console.log(g());
// setter face + direct call of the same binding
const w = function (v: any) {
  if (this !== undefined) {
    this._w = v + 1;
  }
  return "w:" + v;
};
const p: any = {};
p.__defineSetter__("w", w);
p.w = 41;
console.log(p._w);
console.log(w(5));
// nested-scope mixed profile resolves its local const
function scoped(): any {
  const q = function () {
    if (this === undefined) {
      return 0;
    }
    return this._z;
  };
  const o: any = { _z: 33 };
  o.__defineGetter__("z", q);
  return q() + o.z;
}
console.log(scoped());
