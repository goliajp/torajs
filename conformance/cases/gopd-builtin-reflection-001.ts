// Builtin reflection registry 补齐 (RFC 20260721-object-descriptor-cluster
// 刀 1) — four faces:
// 1. `<Ctor>.prototype.constructor` static value read answers the interned
//    ctor cell (same identity as the bare namespace ident, §20.x.3.1).
// 2. gOPD(<Ctor>, "prototype") answers { w: false, e: false, c: false },
//    value = the builtin prototype singleton (§20.x.2.x).
// 3. gOPD(Number, <constant>) answers the §21.1.2 data constants, all
//    { w: false, e: false, c: false }.
// 4. Object.prototype.isPrototypeOf is an own property of Object.prototype
//    (§20.1.3.3) — readable and gOPD-visible.
const oc: any = Object.prototype.constructor;
console.log(typeof oc, oc === Object);
const dc: any = Date.prototype.constructor;
console.log(dc === Date, dc === Object);
const ac: any = Array.prototype.constructor;
console.log(ac === Array);

const dp: any = Object.getOwnPropertyDescriptor(Object, "prototype");
console.log(dp.writable, dp.enumerable, dp.configurable);
console.log(dp.value === Object.prototype);
console.log(dp.hasOwnProperty("get"), dp.hasOwnProperty("set"));
const dpf: any = Object.getOwnPropertyDescriptor(Function, "prototype");
console.log(dpf.writable, dpf.value === Function.prototype);
const dpd: any = Object.getOwnPropertyDescriptor(Date, "prototype");
console.log(dpd.configurable, dpd.value === Date.prototype);

const nm: any = Object.getOwnPropertyDescriptor(Number, "MAX_VALUE");
console.log(nm.value, nm.writable, nm.enumerable, nm.configurable);
const nn: any = Object.getOwnPropertyDescriptor(Number, "NaN");
console.log(nn.value, nn.configurable);
const ni: any = Object.getOwnPropertyDescriptor(Number, "NEGATIVE_INFINITY");
console.log(ni.value);
const ns: any = Object.getOwnPropertyDescriptor(Number, "MAX_SAFE_INTEGER");
console.log(ns.value);
const nv: any = Object.getOwnPropertyDescriptor(Number, "MIN_VALUE");
console.log(nv.value);
console.log(Object.getOwnPropertyDescriptor(Number, "NO_SUCH_CONST"));

const ip: any = Object.getOwnPropertyDescriptor(Object.prototype, "isPrototypeOf");
console.log(typeof ip.value, ip.writable, ip.enumerable, ip.configurable);
const ipv: any = Object.prototype.isPrototypeOf;
console.log(ip.value === ipv);
