// RFC 20260712-arr-exotic-define chunk B — Array DefineOwnProperty
// kernel: index defines land in element storage with per-index
// attribute shadow flags; redefines validate per spec 10.1.6.3.
let a = [];
Object.defineProperty(a, "0", { value: 42, enumerable: true });
console.log(a[0], a.length);
let d: any = Object.getOwnPropertyDescriptor(a as any, "0");
console.log(d.value, d.writable, d.enumerable, d.configurable);
try {
  Object.defineProperty(a, "0", { value: 43 });
  console.log("no throw");
} catch (e) {
  console.log("redefine-value threw:", e instanceof TypeError);
}
try {
  Object.defineProperty(a, "0", { enumerable: false });
  console.log("no throw");
} catch (e) {
  console.log("enum-flip threw:", e instanceof TypeError);
}
// same-value redefine on a readonly property is allowed
Object.defineProperty(a, "0", { value: 42 });
console.log("same-value ok:", a[0]);
// generic descriptor creates the property with value undefined
let b = [];
Object.defineProperty(b, "0", { enumerable: true });
console.log(b[0], b.length);
let db: any = Object.getOwnPropertyDescriptor(b as any, "0");
console.log(db.value, db.writable, db.enumerable, db.configurable);
// -0 / +0 SameValue distinction
let c = [];
Object.defineProperty(c, "0", { value: -0 });
let dc: any = Object.getOwnPropertyDescriptor(c as any, "0");
console.log(dc.value, 1 / dc.value);
try {
  Object.defineProperty(c, "0", { value: +0 });
  console.log("no throw");
} catch (e) {
  console.log("zero-sign threw:", e instanceof TypeError);
}
// fully-default flags keep the array non-exotic
let e2 = [1, 2];
Object.defineProperty(e2, "1", { value: 9, writable: true, enumerable: true, configurable: true });
console.log(e2[1], e2.length);
let de2: any = Object.getOwnPropertyDescriptor(e2 as any, "1");
console.log(de2.value, de2.writable, de2.enumerable, de2.configurable);
// runtime-descriptor path (verifyProperty restore shape)
let f = [5];
let o: any = f;
let dyn: any = { value: 7, writable: true, enumerable: true, configurable: true };
Object.defineProperty(o, "0", dyn);
console.log(f[0]);
// append at len grows the array
Object.defineProperty(f, "1", { value: 11, writable: true, enumerable: true, configurable: true });
console.log(f[1], f.length);
