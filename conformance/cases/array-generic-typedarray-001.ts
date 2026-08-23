// §23.1.3 intentionally-generic Array methods over a TypedArray
// receiver — the generic scan reads `length` (0 for an
// out-of-bounds view) instead of ValidateTypedArray-throwing.
const rab = new ArrayBuffer(4, { maxByteLength: 8 });
const fixed = new Uint8Array(rab, 0, 4);
fixed[0] = 9; fixed[1] = 2;
console.log(Array.prototype.find.call(fixed, (n: any) => n == 2));
const f: any = Array.prototype.find;
console.log(f.call(fixed, (n: any) => n == 9));
rab.resize(2);
console.log(Array.prototype.find.call(fixed, (n: any) => true));
console.log(Array.prototype.indexOf.call(fixed, 0));
console.log(Array.prototype.includes.call(fixed, 9));
console.log(f.call(fixed, (n: any) => true));
const fe: any = Array.prototype.forEach;
let cnt = 0;
fe.call(fixed, () => { cnt++; });
console.log(cnt);
const arr = [1, 2, 3];
console.log(Array.prototype.find.call(arr, (n: any) => n > 1));
console.log(Array.prototype.map.call(arr, (n: any) => n * 2));
console.log(Array.prototype.join.call(arr, "-"));
const obj = { 0: "a", 1: "b", length: 2 };
console.log(Array.prototype.join.call(obj, "+"));
