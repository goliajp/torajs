// A choice among named functions handed to a closure-typed parameter
// through a member call: the argument is the cell of whichever
// function the choice picked.
function f1(v: number) {
  console.log("f1", v);
}
function f2(v: number) {
  console.log("f2", v);
}
function f3(v: number) {
  console.log("f3", v);
}
let t = (f: (v: number) => void) => f(1);
const o = { k: t };
o.k((v: number) => console.log("arrow", v));
for (const c of [true, false]) o.k(c ? f1 : f2);
for (const n of [0, 1, 2]) o.k(n === 0 ? f1 : n === 1 ? f2 : f3);
o.k(f3 || f1);
o.k((console.log("side"), f2));
