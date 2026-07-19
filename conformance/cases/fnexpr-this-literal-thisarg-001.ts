// Array HOF thisArg passed as an INLINE object literal: the
// literal's owned temp is the this-box's only stake (box_to_any is a
// pure encoding), so it must release after the loop — the historical
// pre-loop release freed the payload after iteration 1 and
// `this.mul` read garbage from the second element on.
const arr = [1, 2, 3];
const out: any = [];
arr.forEach(function (x: number) {
  out.push(x * this.mul);
}, { mul: 10 });
console.log(out.join(","));
const doubled = arr.map(function (x: number) {
  return x + this.off;
}, { off: 5 });
console.log(doubled.join(","));
const kept = arr.filter(function (x: number) {
  return x >= this.min;
}, { min: 2 });
console.log(kept.join(","));
