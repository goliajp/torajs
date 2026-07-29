// ES §13.4.4 / §13.4.5 — `x++` and `x--` over a slot typed `any`.
//
// The typed lane adds one at the slot's own width with nothing in
// between, so it demanded a Number target and an `any` slot was
// rejected outright. But §13.4.4.1 puts a ToNumeric before the add:
// the operand may be a string, a boolean, or undefined, and whichever
// it is decides the numeric domain the step happens in — a BigInt
// increments as a BigInt and must not be mixed with a Number one.
//
// The coercion must also run exactly once (a second one would call
// `valueOf` twice, which is observable), so the whole read-modify-write
// is one runtime step over the slot rather than a load and a store
// with lowering in between.

// a `var` binding types as `any` — it hoists to function scope, so the
// declaration and the initializer are not the same event
var i = 0;
i++;
console.log(i);
i--;
console.log(i);

// the expression answers the COERCED old value, not the original
var s = "5";
console.log(s++);
console.log(s);

var b = true;
console.log(b++, b);

// undefined coerces to NaN, and NaN + 1 is NaN
var u;
console.log(u++, u);

// a string that does not parse as a number gives NaN too
var z = "abc";
z++;
console.log(z);

// BigInt stays in its own domain in both directions
var g = 9007199254740993n;
console.log(g++, g);
var n = 5n;
console.log(n--, n);

// the slot survives capture and repeated stepping
var m = 0;
function bump() {
  m++;
}
bump();
bump();
console.log(m);

var k = 0;
for (var t = 0; t < 3; t++) {
  k++;
}
console.log(k, t);
