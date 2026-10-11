// a default reading a binding still in its temporal dead zone throws when
// the function is called early (ECMA-262 §9.1.1.1.1 GetBindingValue,
// §10.2.11), and reads it normally once the declaration has run
try {
  f();
} catch (e) {
  console.log("early call:", e instanceof ReferenceError);
}
const K = 1;
function f(y: number = K) {
  return y;
}
console.log("late call:", f(), f(5));

const g = (y: number = K2) => y;
try {
  g();
} catch (e) {
  console.log("arrow early:", e instanceof ReferenceError);
}
const K2 = 2;
console.log("arrow late:", g());

function h(y: number = K3): number {
  return y;
}
function callH(): number {
  return h();
}
try {
  callH();
} catch (e) {
  console.log("through a caller:", e instanceof ReferenceError);
}
let K3 = 3;
console.log("through a caller, late:", callH(), h());

function m(a: any = 1, b: number = K4) {
  return a + b;
}
try {
  m();
} catch (e) {
  console.log("after a guarded default:", e instanceof ReferenceError);
}
const K4 = 4;
console.log("after a guarded default, late:", m(), m(2), m(undefined, 5));
