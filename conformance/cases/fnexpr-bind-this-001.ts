// A this-reading fn-expr as a `.bind` receiver — §22.2.3.2 stores
// the thisArg as [[BoundThis]]; the module-goal call does not
// ToObject it (§10.2.1.2), so primitives come back untouched.
var obj = { prop: "a" };
var func = function () {
  return this;
};
var newFunc = func.bind(obj);
console.log(newFunc() === obj);

// The test262 spelling routes through Function.prototype.bind.call —
// the receiver rides the first argument.
var func2 = function () {
  return this;
};
var b2 = Function.prototype.bind.call(func2, obj);
console.log(b2() === obj);

// Primitive [[BoundThis]] values come back untouched.
var func3 = function () {
  return this;
};
console.log(func3.bind(42)());
console.log(func3.bind("str")());
console.log(func3.bind(true)());
console.log(func3.bind(null)());
console.log(func3.bind(undefined)());

// A partial application alongside the receiver slot.
var func4 = function (x, y) {
  return this.base + x + y;
};
var b4 = func4.bind({ base: 100 }, 20);
console.log(b4(3));

// Untouched faces: a this-free bind keeps its existing lane…
var plain = function (x, y) {
  return x + y;
};
console.log(plain.bind(null, 1)(2));

// …and a this-using NAMED fn's bind stays on the knife-4 path.
function named() {
  return this.x;
}
console.log(named.bind({ x: 5 })());
