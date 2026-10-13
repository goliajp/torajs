// A rest parameter with no annotation takes the array type a variadic
// target declares for it (`(...xs: number[]) => R` makes `xs` a
// `number[]`), at every place a target type reaches an arrow. It used
// to stay `any[]` and the arrow was refused as not matching its target.
function call(h: (...xs: number[]) => number): number {
  return h(4, 5);
}
console.log(call((...xs) => xs.length));
console.log(call((...xs) => xs.reduce((a, b) => a + b, 0)));

// fixed parameters against a variadic target take its element type
console.log(call((a, b) => a + b));
console.log(call((a, ...r) => a * 10 + r.length));
console.log(call((a) => a));

const p: (a: string, ...rest: number[]) => string = (a, ...rest) => a + rest.join(",");
console.log(p("x", 1, 2));
console.log(p("y"));

function mk(k: number): (...xs: number[]) => number {
  return (...xs) => xs.length * k;
}
console.log(mk(3)(1, 2));

type O = { f: (...xs: string[]) => string };
const o: O = { f: (...xs) => xs.join("-") };
console.log(o.f("a", "b", "c"));

const arr: Array<(...xs: number[]) => number> = [(...xs) => Math.max(...xs)];
arr.push((...xs) => xs[0] ?? -1);
console.log(arr[0](3, 9, 2), arr[1](), arr[1](8));
