// RFC 20260721-string-proto-cluster 刀 5 (G5 可执行面) —
// §22.1.3.28/.35 thisStringValue: a String-prototype-minted
// toString/valueOf borrowed onto a non-string receiver throws;
// primitive-wrapper console.log inspect prints bun's
// [String: "…"] / [Number: n] / [Boolean: b] forms (nested
// StringWrapper prints just the quoted data).
try {
  (String.prototype.valueOf as any).call(true);
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}
try {
  (String.prototype.toString as any).call(5);
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}
console.log((String.prototype.valueOf as any).call("ok"));
const obj: any = { valueOf: function () {}, toString: void 0 };
const s: any = new String(obj);
console.log("newstr:", s);
console.log(new Number(7));
console.log(new Boolean(false));
console.log(new String("hi"));
console.log([new String("a"), new Number(1), new Boolean(true), 2]);
