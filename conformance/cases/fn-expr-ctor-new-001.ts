// RFC 20260726-new-on-function blade A (rotation 234) — `new F()`
// where F is a function EXPRESSION bound to a let/var/const, the
// census's main constructor shape (`var Con = function() {};`).

var Con = function () {};
var child = new Con();
console.log(typeof child);

// The test262 idiom: a prototype is assigned, then the instance's
// class is probed. The instance is a plain dynamic object, so
// Array.isArray answers false even with an array prototype.
var proto = [];
var C2 = function () {};
C2.prototype = proto;
var kid = new C2();
console.log(Array.isArray(kid));

// let / const bindings construct the same way var does.
let L = function () {};
console.log(typeof new L());
const K = function () {};
console.log(typeof new K());

// Parameters forward through the factory; the body may use them
// without mentioning `this`.
var seen: number[] = [];
var Rec = function (a: number, b: number) {
  seen.push(a + b);
};
new Rec(3, 4);
new Rec(10, 20);
console.log(seen[0], seen[1]);

// Untyped params forward as dynamic values.
var Note = function (m) {
  console.log("note:", m);
};
new Note("hi");

// §10.2.2 step 8 — a returned object wins over the fresh receiver...
var Rich = function () {
  return { y: 2 };
};
console.log(new Rich().y);

// ...a returned primitive does not.
var Poor = function () {
  return 42;
};
console.log(typeof new Poor());

// Instances are independent.
var Box = function () {};
var b1 = new Box();
var b2 = new Box();
b1.tag = "one";
b2.tag = "two";
console.log(b1.tag, b2.tag);

// A function declaration of the same shape still works (blade 2).
function DeclCon() {}
console.log(typeof new DeclCon());
