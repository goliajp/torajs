// §10.4.4 — an arguments object's `length` is a PLAIN data property
// ({writable: true, enumerable: false, configurable: true}), unlike
// an Array's §10.4.2 non-configurable exotic length. The materialized
// `__torajs_arguments` cell carries FLAG_ARR_ARGUMENTS (stamped
// right after the mint); the keyed readers — gOPD, delete,
// hasOwnProperty — gate on it, and a delete leaves the element-domain
// hole tombstone under the "length" key (t262 10.6-6-2 / 10.6-7-1).
function del(obj: any, name: string): boolean {
  delete obj[name];
  if (obj.hasOwnProperty(name)) {
    return false;
  }
  return true;
}
function t() {
  var d: any = Object.getOwnPropertyDescriptor(arguments, "length");
  console.log(d.value, d.writable, d.enumerable, d.configurable);
  console.log(del(arguments, "length"));
}
t(7, 8, 9);
var argObj: any = (function () {
  return arguments;
})(1, 2);
var d2: any = Object.getOwnPropertyDescriptor(argObj, "length");
console.log(d2.value, d2.configurable);
console.log(del(argObj, "length"));
var plain: any = [1, 2];
var d3: any = Object.getOwnPropertyDescriptor(plain, "length");
try {
  del(plain, "length");
  console.log(d3.configurable, "no-throw");
} catch (e) {
  console.log(d3.configurable, "TypeError");
}
