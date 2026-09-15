// An accessor's property key and its getter's `name` are two strings:
// reading `get.name` first must not change what the own-name listing
// says (the key cell used to share the getter-name cell's slot).
const d: any = Object.getOwnPropertyDescriptor(Map.prototype, "size");
console.log(d.get.name, Object.getOwnPropertyNames(Map.prototype).includes("size"));
const s: any = Object.getOwnPropertyDescriptor(Symbol.prototype, "description");
console.log(s.get.name, Object.getOwnPropertyNames(Symbol.prototype).includes("description"));
const P: any = Object.getPrototypeOf(Int8Array).prototype;
const bl: any = Object.getOwnPropertyDescriptor(P, "byteLength");
console.log(bl.get.name, Object.getOwnPropertyNames(P).includes("byteLength"), Object.getOwnPropertyNames(P).includes("get byteLength"));
console.log(Object.getOwnPropertyNames(ArrayBuffer.prototype).filter(k => k.startsWith("get ")).length);
