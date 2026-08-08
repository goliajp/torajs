// RFC 20260801-ns-object-value — expando writes on the
// singleton-backed namespaces (`Math.length = 1`, `Math[0] = 1`) are
// ordinary dynobj stores, which unlocks the test262 pattern of using
// Math as an array-like receiver for the generic Array.prototype
// methods.
Math.length = 1;
Math[0] = 1;
var m: any = Math;
console.log(m.length, m[0]);
function callbackfn(val: any, idx: any, obj: any) {
  return "[object Math]" !== Object.prototype.toString.call(obj);
}
console.log(Array.prototype.every.call(Math, callbackfn));
JSON.marker = 7;
var j: any = JSON;
console.log(j.marker);
