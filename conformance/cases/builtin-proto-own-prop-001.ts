// Builtin `<Ctor>.prototype` own-property probe (RFC 20260712
// chunk 1) — interned builtin methods count as the prototype's own
// properties; the universal Object.prototype probes stay inherited
// everywhere but on Object.prototype itself.

console.log(Object.prototype.hasOwnProperty.call(String.prototype, "small"));
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "anchor"));
console.log(Object.prototype.hasOwnProperty.call(Number.prototype, "toFixed"));
console.log(Object.prototype.hasOwnProperty.call(Number.prototype, "small"));
console.log(Object.prototype.hasOwnProperty.call(Date.prototype, "getYear"));
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "hasOwnProperty"));
console.log(Object.prototype.hasOwnProperty.call(Object.prototype, "hasOwnProperty"));
// own but not enumerable
console.log(String.prototype.propertyIsEnumerable("small"));
console.log(Object.keys(String.prototype).length);
// a monkey-patched own entry stays first-class
const sp: any = String.prototype;
sp.zzz = 1;
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "zzz"));
console.log(Object.prototype.hasOwnProperty.call(Function.prototype, "bind"));
console.log(Object.prototype.hasOwnProperty.call(Map.prototype, "get"));
