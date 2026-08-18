const parts: any = [2, 3];
const obj: any = new (function (this: any, a: any, b: any, c: any) {
  this.sum = a + b + c;
})(1, ...parts);
console.log(obj.sum);
const only: any = new (function (this: any, x: any, y: any) {
  this.n = x + y;
})(...parts);
console.log(only.n);
const iter: any = {};
iter[Symbol.iterator] = function (): any {
  let count = 3;
  return {
    next: function (): any {
      count = count + 1;
      return { done: count === 6, value: count };
    },
  };
};
const five: any = new (function (this: any, a: any, b: any, c: any, d: any, e: any) {
  this.last = e;
  this.fourth = d;
})(1, 2, 3, ...iter);
console.log(five.fourth, five.last);
try {
  const n: any = 5;
  const bad: any = new (function (this: any) {})(...n);
  console.log(bad);
} catch (err) {
  console.log("caught");
}
try {
  const nc: any = 7;
  const bad2: any = new nc(...parts);
  console.log(bad2);
} catch (err) {
  console.log("caught2");
}
console.log("done");
