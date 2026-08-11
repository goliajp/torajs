// rotation 365 — a harness-style fn whose PARAM shares a top-level
// binding's name (and is captured by its own inner closure — the
// t262 __t262_throwsAsync(func) idiom) no longer kills the argv
// chain: the shadow-aware walk exempts the shadowing body's uses
// unconditionally (knife-7 gate lifted), and the admitted top-level
// binding's direct call rides the boxed dual entry through the
// globals-fallback variadic route (the let-decl variadic_locals lane
// never sees a promoted top-level binding).
function unrelated(func: any): any {
  return new Promise(function (resolve: any): void {
    resolve(func());
  });
}
var called = 0;
function callbackfn(val: any, idx: any, obj: any) {
  called++;
  return val === 11;
}
var func = function (a: any, b: any) {
  return Array.prototype.every.call(arguments, callbackfn);
};
console.log(func(11));
console.log(called);
console.log(func(11, 11, 11));
console.log(called);
