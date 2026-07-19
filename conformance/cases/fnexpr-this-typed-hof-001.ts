// fn-expr `this` as a TYPED-array HOF callback with a thisArg (RFC
// 20260717-fnexpr-this-channel knife 4, typed-receiver slice): the
// inlined forEach / map / filter loops thread the boxed thisArg as
// the promoted callback's leading arg
const arr = [1, 2, 3];
const ctx = { mul: 10 };
const out: any = [];
arr.forEach(function (x: number) { out.push(x * this.mul); }, ctx);
console.log(out.join(","));

const m = arr.map(function (x: number) { return x + this.mul; }, ctx);
console.log(m.join(","));

const f = arr.filter(function (x: number) { return x >= this.min; }, { min: 2 });
console.log(f.join(","));

// this-free callbacks keep the plain devirt path; reduce is
// initialValue-shaped and never promotes
const plain = arr.map(function (x: number) { return x * 2; });
console.log(plain.join(","));
const acc = arr.reduce(function (a: number, b: number) { return a + b; }, 0);
console.log(acc);
