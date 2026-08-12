// NewDynamic inline-callee face: `new (function () { ... this ... })`
// binds `this` to the freshly allocated object (§10.2.2), and the
// ctor's object return value is the construct result.
var obj = new (function () {
  return this;
});
console.log(typeof obj, obj === globalThis);

// throw-operand face: the thrown closure rides the any-shaped
// exception channel; a detached `e()` binds globalThis (sloppy).
var result: any = "";
try {
  throw function () {
    (this as any).__thrown_mark = "t";
  };
} catch (e) {
  e();
  result = (globalThis as any).__thrown_mark;
}
console.log(result);
