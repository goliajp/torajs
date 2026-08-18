// Spread-site anon argv face — an inline fn-expr called/constructed
// at a spread-carrying site reads `arguments` through the boxed dual
// entry's real argv (the spread kernels' channel), so length and
// element reads answer the runtime argument list (§13.3.8.1).
const parts: any = [2, 3];
const obj: any = new (function (this: any, a: any, b: any, c: any) {
  this.sum = a + b + c;
  console.log(arguments.length);
})(1, ...parts);
console.log(obj.sum);
// spread-only argument list
const only: any = new (function (this: any) {
  this.n = arguments.length;
  console.log(arguments[0], arguments[1]);
})(...parts);
console.log(only.n);
// custom-iterator spread source (§7.4.2 GetIterator at runtime)
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
const five: any = new (function (this: any) {
  this.len = arguments.length;
  console.log(arguments[3], arguments[4]);
})(1, 2, 3, ...iter);
console.log(five.len);
// non-iterable spread source throws catchably
try {
  const n: any = 5;
  const bad: any = new (function (this: any) {})(...n);
  console.log(bad);
} catch (err) {
  console.log("caught");
}
// non-constructor callee still raises the construct TypeError
try {
  const nc: any = 7;
  const bad2: any = new nc(...parts);
  console.log(bad2);
} catch (err) {
  console.log("caught2");
}
console.log("done");
