// map's callback return is polymorphic at full spec arity — a
// (val, idx, obj) => U callback produces Array<U> (§23.1.3.19);
// trailing thisArg composes with the receiver-first forwarder.
function callbackfn(val, idx, obj) {
  return val > 10;
}
var testResult = [12, 11, 9].map(callbackfn);
console.log(testResult, testResult.length);
function cb0() { return true; }
console.log([1, 2].map(cb0));
var objArray = [1, 2, 3];
function cbThis(val) { return this.length + val; }
console.log([10, 20].map(cbThis, objArray));
console.log([1, 2].map(function (v, i) { return v > i; }));
