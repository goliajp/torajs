// `new f()` on a function a global eval declares evaluates the name
// first: before the eval runs it is a ReferenceError, raised before
// any argument is evaluated. After it, the construction reaches the
// binding.
var log = [];
try {
  new f1(log.push("arg"));
  console.log("constructed");
} catch (e) {
  console.log(e.constructor.name, log.length);
}
try {
  new (f1)();
} catch (e) {
  console.log(e.constructor.name);
}
function early() {
  return new f1(1);
}
try {
  early();
} catch (e) {
  console.log("early", e.constructor.name);
}
eval("function f1(x){ this.a = x }");
f1.prototype.get = function () {
  return this.a + 1;
};
const o = new f1(4);
console.log(o.a, o.get(), o instanceof f1, Object.getPrototypeOf(o) === f1.prototype);
console.log(early().a);
eval("function f2(){}");
console.log(typeof new f2(), new f2() instanceof f2);
