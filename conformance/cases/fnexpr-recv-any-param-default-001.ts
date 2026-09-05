// A `this`-using function expression standing as a DEFAULT PARAMETER
// value used to cost its binding the promoted receiver, so the whole
// program refused to compile.
//
// By the time the receiver census runs, that default is no longer a
// default: `materialize_expr_defaults` has already moved it into the
// body as `if (f === undefined) { f = g }` and left the param's own
// default as the `undefined` pad. So the position that has to be
// answered for is the ASSIGNMENT VALUE, and the slot it lands in is
// the param — which that pass only ever builds a guard for when the
// param is `any` (an unannotated one is widened to `any` on the
// spot). An `any` slot is the any lane, and every any-lane call path
// honours FLAG_CLOSURE_RECV_FIRST.
let ctor = function (this: any) {
  this.q = 1;
  return this;
};

// the default is taken: construct out of it, receiver intact
function build(f: any = ctor) {
  const made: any = new (f as any)();
  return made.q;
}
console.log(build());

// and when the argument IS supplied, the same slot carries it
console.log(build(ctor));

// a plain call through the slot seeds `undefined` (§10.2.1.2)
let probe = function (this: any) {
  return this === undefined;
};
function callIt(f: any = probe) {
  return (f as any)();
}
console.log(callIt(), callIt(probe));

// an UNANNOTATED param is widened to `any` by the same pass
function bare(f = ctor) {
  return typeof f;
}
console.log(bare());

// nested functions and arrows spell the same thing
function outer() {
  function inner(f: any = ctor) {
    return typeof f;
  }
  return inner();
}
console.log(outer());
const asArrow = (f: any = ctor) => typeof f;
console.log(asArrow());

// taking the default more than once keeps the callee alive — the
// premature-free shape the objlit/array fixtures witness
console.log(build(), build(), build());
