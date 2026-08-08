// §23.1.3.1.1 step 5.c.iii — the concat derive writes elements with
// CreateDataPropertyOrThrow (DEFINE semantics): a configurable
// non-writable entry on the species product REDEFINES to the fresh
// value, a non-configurable one refuses with a TypeError. The
// set-semantics shortcut threw on writable:false either way.
var A = function (n: any) {
  Object.defineProperty(this, "0", {
    value: 1,
    writable: false,
    enumerable: false,
    configurable: true,
  });
};
var arr = [];
arr.constructor = {};
arr.constructor[Symbol.species] = A;
var res = arr.concat(2);
console.log(res[0]);
var B = function (n: any) {
  Object.defineProperty(this, "0", {
    value: 1,
    writable: false,
    enumerable: false,
    configurable: false,
  });
};
var arr2 = [];
arr2.constructor = {};
arr2.constructor[Symbol.species] = B;
try {
  arr2.concat(3);
  console.log("no throw");
} catch (e) {
  console.log("TypeError caught");
}
