// Builtin `<Ctor>.prototype` gOPD descriptor synthesis (RFC 20260712
// chunk 2) — interned family methods answer the spec method
// descriptor {writable: true, enumerable: false, configurable: true};
// the any-receiver member read hands out the same immortal cell.
const d1: any = Object.getOwnPropertyDescriptor(String.prototype, "small");
console.log(typeof d1.value, d1.writable, d1.enumerable, d1.configurable);
const d2: any = Object.getOwnPropertyDescriptor(String.prototype, "anchor");
console.log(typeof d2.value, d2.writable, d2.enumerable, d2.configurable);
const d3: any = Object.getOwnPropertyDescriptor(Number.prototype, "toFixed");
console.log(typeof d3.value, d3.writable, d3.enumerable, d3.configurable);
// miss name stays undefined
console.log(Object.getOwnPropertyDescriptor(String.prototype, "nope"));
// wrong-family name stays undefined
console.log(Object.getOwnPropertyDescriptor(Number.prototype, "anchor"));
// identity: descriptor value IS the interned method cell
const sp: any = String.prototype;
console.log(d1.value === sp.small);
// monkey-patched own entry wins over the interned family cell
sp.zzz = 42;
const d4: any = Object.getOwnPropertyDescriptor(String.prototype, "zzz");
console.log(d4.value, d4.writable, d4.enumerable, d4.configurable);
// universal probes are own only on Object.prototype
console.log(Object.getOwnPropertyDescriptor(String.prototype, "hasOwnProperty"));
const d5: any = Object.getOwnPropertyDescriptor(Object.prototype, "hasOwnProperty");
console.log(typeof d5.value, d5.writable, d5.enumerable, d5.configurable);
// Function / Map / Date family spots
const d6: any = Object.getOwnPropertyDescriptor(Function.prototype, "bind");
console.log(typeof d6.value, d6.enumerable);
const d7: any = Object.getOwnPropertyDescriptor(Map.prototype, "get");
console.log(typeof d7.value, d7.configurable);
const d8: any = Object.getOwnPropertyDescriptor(Date.prototype, "getYear");
console.log(typeof d8.value, d8.writable);
