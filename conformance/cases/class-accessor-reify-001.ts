// RFC 20260718-accessor-reify 刀 2 — a class accessor is a real AccessorPair
// own entry on C.prototype: gOPD answers { get, set, E0, C1 }, the faces
// carry "get <p>" / "set <p>" names and spec lengths, and both call routes
// (pair invoke through an any receiver, explicit .call) reach the bodies.
class C {
  private _v: number = 1;
  get x(): number { return this._v; }
  set x(v: number) { this._v = v; }
  get ro(): number { return 42; }
}
const d: any = Object.getOwnPropertyDescriptor(C.prototype, "x");
console.log("has-desc", d !== undefined);
console.log("get-type", typeof d.get, "set-type", typeof d.set);
console.log("enum", d.enumerable, "conf", d.configurable);
console.log("has-value", "value" in d, "has-writable", "writable" in d);
console.log("get-name", d.get.name, "set-name", d.set.name);
console.log("get-len", d.get.length, "set-len", d.set.length);

// .call routes
const c = new C();
console.log("get-call", d.get.call(c));
d.set.call(c, 7);
console.log("after-set", c.x);

// get-only accessor — set face is undefined
const dro: any = Object.getOwnPropertyDescriptor(C.prototype, "ro");
console.log("ro-desc", dro !== undefined);
console.log("ro-get-type", typeof dro.get, "ro-set", dro.set);
console.log("ro-get-call", dro.get.call(c));

// any-lane member read/write still route through the accessor
const ac: any = c;
console.log("any-read", ac.x);
ac.x = 9;
console.log("any-after-set", c.x);

// enumeration posture — E0 keeps keys empty
console.log("keys", JSON.stringify(Object.keys(C.prototype)));
console.log("gopn-has-x", Object.getOwnPropertyNames(C.prototype).indexOf("x") >= 0);

// typed lane unchanged
c.x = 3;
console.log("typed", c.x);
