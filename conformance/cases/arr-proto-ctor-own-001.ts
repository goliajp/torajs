// RFC 20260721 刀 11 G11 — `constructor` is an own data property of
// every builtin prototype (§20.x.3.1), delete-tombstoned through its
// slot bit; the builtin ctor's `prototype` is {[[Writable]]: false,
// [[Configurable]]: false} (§22.1.2.4 family), so the strict write
// and the delete both throw.
console.log(Array.prototype.hasOwnProperty("constructor"));
console.log(Array.prototype.constructor === Array);
console.log(String.prototype.hasOwnProperty("constructor"));
console.log(Number.prototype.constructor === Number);
let p: any = Number.prototype;
console.log("del:", delete p.constructor);
console.log("own after del:", p.hasOwnProperty("constructor"));
let a: any = Array;
try {
  a.prototype = 5;
  console.log("write ok");
} catch (e) {
  console.log("write threw");
}
try {
  delete a.prototype;
  console.log("delete ok");
} catch (e) {
  console.log("delete threw");
}
console.log("proto intact:", Array.prototype.constructor === Array);
