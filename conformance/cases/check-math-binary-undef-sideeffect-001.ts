// S274 — Math.{min,max,pow,atan2,imul} S228 NaN/0 fold preserves
// side effects of non-undef args. Pre-S274 the SSA-emit short-
// circuit returned the spec constant without lowering any arg, so
// step()-style trailing exprs silently dropped (calls counter
// would read 0 in tora, non-zero in bun).

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

console.log(Math.min(undefined, step(2), step(3)));
console.log(Math.min(step(1), undefined, step(3)));
console.log(Math.min(step(1), step(2), undefined));

console.log(Math.max(undefined, step(5)));
console.log(Math.max(step(10), undefined));

console.log(Math.pow(step(2), undefined));
console.log(Math.pow(undefined, step(3)));
console.log(Math.atan2(step(1), undefined));
console.log(Math.atan2(undefined, step(2)));
console.log(Math.imul(step(7), undefined));
console.log(Math.imul(undefined, step(11)));

console.log("calls=" + calls);
