// concat copies its container arguments into a product of the receiver's
// class; the classes must be joined whether or not the result is bound.
const a = [1.5, 2.5];
console.log(a.concat([9]));
const b: number[] = [1.5];
const c: number[] = [9, 10];
console.log(b.concat(c));
console.log([0.5].concat([1, 2], [3.5]));
const d = [1, 2];
console.log(d.concat(1.5), d.concat([2.5]), d);
const e = [3, 4];
console.log(e.concat(5, [6]), e[0] + 1);
