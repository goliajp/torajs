// RFC 20260717-fnexpr-this-channel knife 2W cut 1 — one fn-expr
// closure reused as an accessor face on several objects: every use of
// the binding is a face read, so the shared closure promotes and each
// face invoke binds `this` to its own receiver.
const g = function () {
  return this._x;
};
const o1: any = { _x: 11 };
const o2: any = { _x: 22 };
o1.__defineGetter__("x", g);
o2.__defineGetter__("x", g);
console.log(o1.x);
console.log(o2.x);
// shared setter face
const s = function (v) {
  this._y = v * 3;
};
const a: any = {};
const b: any = {};
a.__defineSetter__("y", s);
b.__defineSetter__("y", s);
a.y = 2;
b.y = 5;
console.log(a._y);
console.log(b._y);
// shared face through the literal-descriptor path
const rd = function () {
  return this._z + 1;
};
const c: any = { _z: 30 };
const d: any = { _z: 40 };
Object.defineProperty(c, "z", { get: rd });
Object.defineProperty(d, "z", { get: rd });
console.log(c.z);
console.log(d.z);
// one binding across BOTH face position kinds (legacy define +
// literal descriptor)
const m = function () {
  return this._w * 2;
};
const e: any = { _w: 4 };
const f: any = { _w: 6 };
e.__defineGetter__("w2", m);
Object.defineProperty(f, "w2", { get: m });
console.log(e.w2);
console.log(f.w2);
