// fn-expr `this` in nested literal descriptors (RFC
// 20260717-fnexpr-this-channel knife 5): Object.defineProperties and
// Object.create descriptor-of-descriptors faces bind `this` to the
// property receiver
const o: any = {};
Object.defineProperties(o, {
  y: {
    get: function () { return this._y ?? 7; },
    set: function (v) { this._y = v * 2; },
  },
});
o.y = 21;
console.log(o.y);

const p: any = Object.create(null, {
  z: { get: function () { return 99; } },
});
console.log(p.z);

const q: any = Object.create({ inherited: 1 }, {
  w: {
    get: function () { return this._w ?? "unset"; },
    set: function (v) { this._w = "[" + v + "]"; },
  },
  plain: { value: 5, enumerable: true },
  nothis: { get: function () { return 42; } },
});
console.log(q.w);
q.w = "x";
console.log(q.w);
console.log(q.plain, q.nothis, q.inherited);

const o2: any = {};
Object.defineProperties(o2, {
  a: { get: function () { return this.b ?? 0; } },
  b: { value: 9, writable: true },
});
console.log(o2.a, o2.b);
