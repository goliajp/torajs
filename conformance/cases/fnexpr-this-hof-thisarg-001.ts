// fn-expr `this` as an array-HOF callback with a thisArg (RFC
// 20260717-fnexpr-this-channel knife 4): the any-lane kernels seed
// argv[0] with T per §23.1.3, undefined when absent
const arr: any = [1, 2, 3];
const ctx = { mul: 10 };
const out: any = [];
arr.forEach(function (x: any) { out.push(x * this.mul); }, ctx);
console.log(out.join(","));

const m = arr.map(function (x: any) { return x + this.mul; }, ctx);
console.log(m.join(","));

console.log(arr.filter(function (x: any) { return x >= this.min; }, { min: 2 }).join(","));
console.log(arr.find(function (x: any) { return x > this.min; }, { min: 1 }));
console.log(arr.some(function (x: any) { return x === this.t; }, { t: 3 }));
console.log(arr.every(function (x: any) { return x < this.cap; }, { cap: 10 }));
console.log(arr.findIndex(function (x: any) { return x === this.t; }, { t: 2 }));
console.log(arr.findLast(function (x: any) { return x < this.cap; }, { cap: 3 }));
console.log(arr.findLastIndex(function (x: any) { return x < this.cap; }, { cap: 3 }));
console.log(arr.flatMap(function (x: any) { return [x, x * this.mul]; }, { mul: 100 }).join(","));

// this-free callbacks keep the plain ABI; no-thisArg fn-expr binds
// this = undefined (strict)
const noThis = arr.map(function (x: any) { return x * 2; });
console.log(noThis.join(","));
const thisUndef = arr.map(function (x: any) { return this === undefined ? x : -1; });
console.log(thisUndef.join(","));
