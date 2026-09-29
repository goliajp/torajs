// a named function is hoisted, so a call ahead of a let / const
// declaration reaches its body while the binding is still in its dead
// zone (§8.1.1.1 step 4); the read raises ReferenceError, and the
// same function answers once the declaration has run
function f() {
  return typeof x;
}
try {
  console.log(f());
} catch (e) {
  console.log(e instanceof ReferenceError, (e as Error).message);
}
let x = 1;
console.log(f());
// reached through a closure value
function g() {
  return y + 1;
}
const h = () => g();
try {
  console.log(h());
} catch (e) {
  console.log("h", e instanceof ReferenceError);
}
const y = 41;
console.log(h());
// a write ahead of the declaration
function w() {
  z = 5;
}
try {
  w();
} catch (e) {
  console.log("w", (e as Error).message);
}
let z = 0;
w();
console.log(z);
// reached transitively: a calls b, b reads q
function a() {
  return b();
}
function b() {
  return q;
}
try {
  console.log(a());
} catch (e) {
  console.log("a", e instanceof ReferenceError);
}
const q = "q";
console.log(a());
// the same shape inside a function body
function outer() {
  function inner() {
    return typeof v;
  }
  try {
    console.log(inner());
  } catch (e) {
    console.log("inner", e instanceof ReferenceError);
  }
  const v = "v";
  return inner();
}
console.log(outer());
// a function only called after the declaration keeps answering
function later() {
  return tail * 2;
}
const tail = 21;
console.log(later());
