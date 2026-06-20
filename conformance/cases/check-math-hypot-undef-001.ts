// S271 — Math.hypot accepts Undefined args per ES §21.3.2.18:
// each arg is ToNumber'd, ToNumber(undefined)=NaN, and the
// sum-of-squares + sqrt containing NaN returns NaN. tora's
// check.rs previously rejected Undefined with "Math.hypot args
// must be number, got Undefined"; SSA-emit now folds to NaN
// after eval-and-dropping the non-undef args so trailing side-
// effect expressions still fire.

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

console.log(Math.hypot());
console.log(Math.hypot(3));
console.log(Math.hypot(3, 4));
console.log(Math.hypot(3, 4, 12));

// 1+ undefined → NaN.
console.log(Math.hypot(undefined));
console.log(Math.hypot(3, undefined));
console.log(Math.hypot(undefined, 4));
console.log(Math.hypot(undefined, undefined));

// Mixed with side-effect exprs — non-undef positions must eval.
console.log(Math.hypot(step(5), undefined, step(7)));
console.log(Math.hypot(undefined, step(11)));
console.log(Math.hypot(step(13), step(17), undefined));

console.log("calls=" + calls);
