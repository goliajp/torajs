// §9.1.9 through a Super Reference — the BASE's chain decides whether
// a setter runs; a plain data write stores onto the RECEIVER.
const parent: any = { _x: 0, base: "b" };
const obj: any = {
  m(v: any) {
    super.base = v;
    return [Object.prototype.hasOwnProperty.call(obj, "base"), obj.base, parent.base];
  },
};
Object.setPrototypeOf(obj, parent);
console.log(JSON.stringify(obj.m("own")));

// An inherited setter runs with `this` as receiver, so the property
// it writes lands on the receiver and not on the prototype.
const proto2: any = { _y: 0, set y(v: any) { this._y = v; } };
const o2: any = { set y(v: any) { super.y = v; } };
Object.setPrototypeOf(o2, proto2);
o2.y = 1;
console.log(o2._y, Object.getPrototypeOf(o2)._y);

// The computed spelling takes the same route.
const proto3: any = { _z: 0, set z(v: any) { this._z = v; } };
const o3: any = { go(k: string, v: any) { super[k] = v; } };
Object.setPrototypeOf(o3, proto3);
o3.go("z", 42);
console.log(o3._z, proto3._z);

// An assignment expression answers the value it stored.
const p4: any = {};
const o4: any = { go() { return (super.q = 7); } };
Object.setPrototypeOf(o4, p4);
console.log(o4.go(), o4.q);
