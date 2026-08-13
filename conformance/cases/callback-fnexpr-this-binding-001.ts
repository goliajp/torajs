// Receiver certainty through a `const` binding — the ordinary way
// these calls are written. `const` cannot be reassigned, so proving
// the initializer proves every read; a `let` of the same name
// anywhere disqualifies it and the callback keeps its loud reject.

const p = Promise.resolve(1);
p.then(function (v: any) {
  console.log("bound-promise", typeof this, v);
});

const chained = p.then(function (v: any) {
  return v + 1;
});
chained.then(function (v: any) {
  console.log("bound-chain", typeof this, v);
});

// Explicitly annotated array bindings count too: the annotation makes
// the receiver MORE certain, not less.
const nums: number[] = [3, 1, 2];
nums.sort(function (a: number, b: number) {
  console.log("annotated-sort", typeof this);
  return a - b;
});
console.log(nums.join(","));

const anys: any[] = [1];
anys.forEach(function (v: any) {
  console.log("annotated-forEach", typeof this, v);
});
