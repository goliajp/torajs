// String.prototype.match / search / split / replace look up the
// pattern's symbol method only when the pattern is an Object; a
// primitive pattern goes straight to the coerce lane, whatever its
// prototype carries (a RegExp protocol method, a user function or a
// getter). A String wrapper object is an Object and is asked.
let reads = 0;
let calls = 0;
const P = RegExp.prototype as any;
(String.prototype as any)[Symbol.match] = P[Symbol.match];
(String.prototype as any)[Symbol.search] = P[Symbol.search];
(Number.prototype as any)[Symbol.replace] = P[Symbol.replace];
Object.defineProperty(String.prototype, Symbol.split, {
  get() {
    reads++;
    return function () {
      calls++;
      return "custom";
    };
  },
  configurable: true,
});
(Number.prototype as any)[Symbol.split] = function () {
  calls++;
  return "custom";
};
(String.prototype as any)[Symbol.replace] = function () {
  calls++;
  return "custom";
};
const x: any = "x";
const five: any = 5;
const dash: any = "-";
console.log(x.match(x));
console.log(("ax" as any).search(x));
console.log(("x5y" as any).replace(five, "-"), ("a-b" as any).replace(dash, "+"));
console.log(("a-b" as any).split(dash), ("x5y" as any).split(five), reads, calls);
console.log("a-b".split(dash), "a-b".replace(dash, "+"), "x5y".split(five), reads, calls);
console.log("x".match(x), "ax".search(x));
console.log("a-b".split("-" as any), "a-b".replace("-" as any, "+"), reads, calls);
console.log("x".match("x" as any), "ax".search("x" as any), "x5y".split(5 as any));
console.log(("a-b" as any).split(Object("-")), reads, calls);
