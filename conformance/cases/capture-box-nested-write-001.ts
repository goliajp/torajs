// A closure body that mints a NESTED closure over the same captured
// name must not release the byref capture box it only borrowed (the
// env owns that stake) — pre-fix the box freed early and every
// top-level read after the call answered freed-memory garbage
// (rotation 162 capture-box UAF; test262 defineProperty
// toStringAccessed appeared pair).
let z = 0;
function run(f: any): void {
  f();
}
run(function () {
  const h = function () { z = 33; };
  h();
});
console.log(z);
z = 5;
console.log(z);
run(function () {
  const k = function () { z = z + 1; };
  k();
  k();
});
console.log(z);

// mint-without-call must not corrupt the binding either
let w = 0;
run(function () {
  const dead = function () { w = 99; };
});
console.log(w);

// the test262 shape: descriptor-value hooks writing captured flags,
// observed after the assert.throws-style callback returns
let a1 = false;
let a2 = false;
run(function () {
  const v = {
    toString: function () { a1 = true; return {}; },
    valueOf: function () { a2 = true; return {}; },
  };
  try {
    Object.defineProperty([], "length", { value: v });
  } catch (e: any) {
    console.log("threw", e instanceof TypeError);
  }
});
console.log(a1, a2);
