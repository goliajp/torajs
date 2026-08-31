// 468-01 remainder: un-annotated top-level object/array literals with
// runtime-expression members promote to the Any lane, so named-fn reads
// resolve (both faces: call-valued fields, nested literals with
// operator-valued fields). Reads from BOTH a named fn and top level —
// the fn face is the registered failing shape.

// face 1: call-valued field
function mk(): any {
  return 7;
}
const o = { v: mk() };
function readO() {
  console.log(o.v);
}
readO();
console.log(o.v);

// face 2: nested literal with an operator-valued field
const n = { a: { b: 1 + 1 }, c: null };
function readN() {
  console.log(n.a.b);
  console.log(n.c);
}
readN();
console.log(n.a.b);

// runtime closure field, called through the any-member lane
function mkFn(): any {
  return () => 42;
}
const cf = { f: mkFn(), tag: "T" };
function callCf() {
  console.log(cf.f());
  console.log(cf.tag);
}
callCf();

// `this` through the any-member call
function mkThisFn(): any {
  return function () {
    return this.label;
  };
}
const tf = { m: mkThisFn(), label: "L" };
function callTf() {
  console.log(tf.m());
}
callTf();

// call-valued element in a mixed array literal
const xs = [1, mk(), "s"];
function readXs() {
  console.log(xs[1]);
  console.log(xs.length);
}
readXs();
console.log(xs[0]);

// nested object literal element carrying an operator-valued field
const ys = [{ b: 2 + 3 }, null];
function readYs() {
  console.log(ys[0].b);
  console.log(ys[1]);
}
readYs();

// alias + shaped operator field through the shared shape table
const base = 10;
const sh = { d: base, e: -4, u: undefined };
function readSh() {
  console.log(sh.d);
  console.log(sh.e);
  console.log(sh.u);
}
readSh();
