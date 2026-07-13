// RFC 20260714-dstr-residual — whole-pattern default on a destructuring
// param (`function f({ a = 1 } = {}) {}`): the un-annotated synth param
// used to pin to the default's inferred type (empty Struct), and the
// desugared pattern reads missed at lower time. The param is now
// any-forced (catch-destr precedent), so ES §10.2.3 default-firing and
// per-field defaults compose.

// function position
function fw({ a = 1 } = {}) {
  console.log("fn:", a);
}
fw();
fw({ a: 9 });

// object-literal method position
const om = {
  m({ a = 2 } = {}) {
    console.log("meth:", a);
  },
};
om.m();
om.m({ a: 10 });

// arrow position
const am = ({ a = 3 } = {}) => {
  console.log("arrow:", a);
};
am();

// array pattern with partial default source
function fx([x = 5, y = 6] = [1]) {
  console.log("arr:", x, y);
}
fx();
fx([7, 8]);
console.log("done");
