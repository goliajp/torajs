// chunk 2.5 F1 (ann-width rfc §5.6) — fn-type annotation width
// negotiation. The `__fn(..)->..` canonical spelling is a nominal
// aggregation point: every annotated slot and every function flowing
// through one (fn idents, closures, forwarders) union their ret /
// param projections, so one f64-possible resident widens the whole
// interned signature while all-integral residents keep the narrow
// ABI. Pre-F1 the annotation parsed its `number` faces straight to
// i64: an indirect call through `pickOp(false)` read the f64 ret off
// the integer register (3 instead of 0.75), and a captured-arrow
// factory truncated through the ann sig (1 instead of 1.5).
function add(x: number, y: number): number { return x + y; }
function half(x: number, y: number): number { return x / y; }
function pickOp(op: boolean): (x: number, y: number) => number {
  if (op) { return add; }
  return half;
}
console.log(pickOp(true)(3, 4));
console.log(pickOp(false)(3, 4));

function makeHalver(d: number): (x: number) => number {
  return (x: number): number => x / d;
}
const h = makeHalver(2);
console.log(h(3));

// un-annotated carrier — flow union alone answers the read
let f = half;
console.log(f(8, 2));

// fract arg through an fn-value reaches the resident's param
function g(x: number): number { return x + 1; }
let gv = g;
console.log(gv(0.5));

// all-int residents hold the narrow signature
function sub(x: number, y: number): number { return x - y; }
function pickInt(op: boolean): (x: number, y: number) => number {
  if (op) { return add; }
  return sub;
}
console.log(pickInt(true)(3, 4));
console.log(pickInt(false)(3, 4));
