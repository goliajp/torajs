// Function("...this...") — 10.2.1.2 sloppy this-bind: a dynamic
// function's this is the global object, so the synthesized body's
// this rewrites to the globalThis singleton; arrows pierce (8.3.4).
var g = Function("return this;")();
console.log(typeof g);
console.log(g === globalThis);
// harness shape: read/write a property through it
(g as any).test262 = 262;
console.log((globalThis as any).test262);
// hint: params + this
var f2 = Function("x", "return this === globalThis && x === 5;");
console.log(f2(5));
// arrow inside body pierces this
var f3 = Function("return (() => this)();");
console.log(f3() === globalThis);
// nested function keeps loud? (behavior: this case must NOT silently mis-answer)
