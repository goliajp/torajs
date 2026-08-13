// The array iteration callbacks the spec invokes with no receiver.
// `sort` / `reduce` never take a thisArg (§23.1.3.30 step 5,
// §23.1.3.24 step 6), and `forEach` / `map` / `filter` take an
// optional one that is absent here — so a function EXPRESSION
// callback sees `this === undefined` in strict code, the same answer
// a named callback already gave.

const nums = [3, 1, 2];
nums.sort(function (a: number, b: number) {
  console.log("sort", typeof this);
  return a - b;
});
console.log(nums.join(","));

console.log(
  [1, 2, 3].reduce(function (acc: number, v: number) {
    console.log("reduce", typeof this);
    return acc + v;
  }, 0),
);

const words = ["a", "b"];
words.forEach(function (w: string) {
  console.log("forEach", typeof this, w);
});

console.log(
  words
    .map(function (w: string) {
      console.log("map", typeof this);
      return w + "!";
    })
    .join(","),
);

// A thisArg IS passed here, so this callback keeps its receiver and
// must NOT be rewritten — it is a named function so the receiver-first
// forwarder handles it, and the expression spelling stays refused
// rather than silently answering undefined.
const host = { tag: "host" };
function named(this: any, w: string): void {
  console.log("thisArg", (this as any).tag, w);
}
words.forEach(named, host);
