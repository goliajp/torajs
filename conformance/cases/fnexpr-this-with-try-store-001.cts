// The receiver censuses walk the shared nested-list spine (rotation
// 437): a `with` inside a `try` desugars to a Block whose `__with_<n>`
// binding the old FnDecl/Block-only recursion never saw, so the
// store-face promote on the guard's `w.f = function () { …this… }` arm
// silently did not fire — the same program worked at top level and
// refused inside `try`.
var myObj: any = { p1: 5 };
var p1: any = "outer";
var st: any = 0;
try {
  with (myObj) {
    var f = function () { st = p1; this.p2 = 88; };
  }
  f();
} catch (e) {}
console.log(st);

// top-level twin — already promoted before the fix; pins no-regression
var obj2: any = { q: 7 };
var q: any = "outer";
var t: any = 0;
with (obj2) {
  var g = function () { t = q; this; };
}
g();
console.log(t);
