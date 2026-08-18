// generator state-machine field annotations for Object.* statics and
// unannotated function calls — both shapes previously took the lift's
// `number` fallback: `let d = Object.getOwnPropertyDescriptor(...)`
// inside a generator printed `0` (and `.configurable` after it was a
// compile reject), and a call to an unannotated function answering a
// symbol stored the result through a number slot (raw pointer error).
var S = Symbol();
function inner() {
  return inner.hasOwnProperty("caller") ? (inner as any).caller : S;
}
var obj = { a: 1 };
var f = function* () {
  let d = Object.getOwnPropertyDescriptor(obj, "a");
  console.log(typeof d, (d as any).configurable);
  let missing = Object.getOwnPropertyDescriptor(obj, "x");
  if (missing) {
    console.log("unexpected");
  } else {
    console.log("missing-undefined");
  }
  let k = Object.keys(obj);
  console.log(k.length, k[0]);
  let r = inner();
  console.log(typeof r);
  yield 1;
};
var it = f();
it.next();
console.log("done");
