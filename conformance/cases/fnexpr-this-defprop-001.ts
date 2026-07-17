const o: any = {};
let outer = 10;
Object.defineProperty(o, "x", {
  get: function () {
    return this._v + outer;
  },
  set: function (nv: any) {
    this._v = nv - 1;
  },
});
o.x = 6;
console.log(o._v);
console.log(o.x);
