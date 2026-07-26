// A number argument has to arrive at the width the closure's parameter
// was compiled for, the way it already did for a named function.
//
// The closure-call lane hand-writes its own argument loop instead of
// going through the direct call's `coerce_args_by_param_tys`, and it
// only ever carried that helper's two `Any` directions. An I64 argument
// into a parameter that some other call site had widened to F64 went in
// raw, and the callee read integer bits out of an f64 slot.
//
// Widening the parameter is what makes two call sites disagree, so the
// narrow call is always the one that breaks — never the call that
// caused the widening. That is why this reads as "the second call
// prints the first call's argument".

const f = (b: number): number => {
  console.log(b);
  return 0;
};
f(6.5);
f(2);

// order reversed — the narrow call comes first
const g = (b: number): number => {
  console.log(b);
  return 0;
};
g(2);
g(6.5);
g(3);

// a prefix parameter alongside the widened one
const h = (a: number, b: number): number => {
  console.log(a, b);
  return 0;
};
h(5, 6.5);
h(1, 2);

// the width can come from anywhere — here from a spread source whose
// element class `find` seeded F64 (it must be able to answer undefined)
const k = (a: number, b: number): number => {
  console.log(a + b);
  return 0;
};
const src: number[] = [7];
src.find((x: number): boolean => x > 0);
k(9, ...src);
k(5, 6);

// a capturing closure takes the same lane
const base: number = 100;
const cap = (b: number): number => {
  console.log(base + b);
  return 0;
};
cap(0.5);
cap(4);

// an all-integral closure stays narrow and unaffected
const narrow = (b: number): number => {
  console.log(b);
  return 0;
};
narrow(1);
narrow(2);
