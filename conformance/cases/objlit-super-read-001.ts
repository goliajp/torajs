// §10.2.4 [[HomeObject]] for object-literal methods — `super.x`
// reads off GetPrototypeOf(home), re-evaluated per access (§13.3.7).
const obj: any = { method() { return super.x; } };
Object.setPrototypeOf(obj, { x: 42 });
console.log(obj.method());

// element form
const o2: any = { m() { return super["k"]; } };
Object.setPrototypeOf(o2, { k: "idx" });
console.log(o2.m());

// an arrow inherits the enclosing method's home (§8.3.4)
const o3: any = { m() { return (() => super.y)(); } };
Object.setPrototypeOf(o3, { y: "arrow" });
console.log(o3.m());

// default [[Prototype]] is %Object.prototype% — miss answers
// undefined, and its own members are readable
const o5: any = { m() { return super.z; } };
console.log(o5.m());
const o6: any = { m() { return super.toString; } };
console.log(typeof o6.m());

// GetSuperBase is not cached — a later setPrototypeOf changes what
// super sees
const proto1: any = { p: "one" };
const proto2: any = { p: "two" };
const sw: any = { m() { return super.p; } };
Object.setPrototypeOf(sw, proto1);
const first = sw.m();
Object.setPrototypeOf(sw, proto2);
console.log(first, sw.m());

// reassigning the declared binding does not move the HomeObject
var vv: any = { m() { return super.q; } };
Object.setPrototypeOf(vv, { q: 1 });
const keep = vv;
vv = { other: true };
console.log(keep.m());
