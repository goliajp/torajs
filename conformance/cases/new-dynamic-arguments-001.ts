// Inline NewDynamic callee joins the static-argv face — `new
// (function () { …arguments[i]… })(…)` resolves the arguments object
// statically from the single construct site (the IIFE knife's twin);
// the construct channel's boxed dual entry feeds every declared
// param, injected extras included.
const obj: any = new (function (this: any) {
  this.n = arguments.length;
  console.log(arguments[0], arguments[1]);
})(7, 8);
console.log(obj.n);
// over-arity past a declared param
const b: any = new (function (this: any, x: any) {
  this.first = x;
  console.log(arguments[2]);
})(1, 2, 3);
console.log(b.first, b.n);
// length-only body stays on its own lane
const c: any = new (function (this: any, x: any) {
  this.n = arguments.length;
})(1, 2, 3);
console.log(c.n);
console.log("done");
