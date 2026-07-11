// Builtin `<Ctor>.prototype` delete tombstone (RFC 20260712 chunk 3)
// — delete hides the interned family method (deleted-mid bitmask on
// torajs-rc, the FLAG_FN_NAME_DELETED precedent generalized); a set /
// defineProperty restore revives because own entries win before the
// intern fallthrough at every station.
const sp: any = String.prototype;
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "small"));
console.log(delete sp.small);
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "small"));
console.log(Object.getOwnPropertyDescriptor(String.prototype, "small"));
console.log(sp.small);
// sibling mid / sibling proto unaffected
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "big"));
console.log(Object.prototype.hasOwnProperty.call(Number.prototype, "toFixed"));
// walks stay ghost-free
console.log(Object.keys(String.prototype).length);
// defineProperty restore revives (entry wins before the tombstone)
Object.defineProperty(String.prototype, "small", {
  value: function () { return "restored"; },
  writable: true,
  enumerable: false,
  configurable: true,
});
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "small"));
const d: any = Object.getOwnPropertyDescriptor(String.prototype, "small");
console.log(typeof d.value, d.writable, d.enumerable, d.configurable);
console.log(sp.small());
// second delete removes the restored entry and re-marks
console.log(delete sp.small);
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "small"));
console.log(sp.small);
// plain-set revive path
sp.small = 7;
console.log(Object.prototype.hasOwnProperty.call(String.prototype, "small"), sp.small);
console.log(delete sp.small);
console.log(sp.small);
// annexB attr form + Date family delete
const dp: any = Date.prototype;
console.log(delete dp.getYear);
console.log(Object.prototype.hasOwnProperty.call(Date.prototype, "getYear"));
console.log(Object.prototype.hasOwnProperty.call(Date.prototype, "setYear"));
