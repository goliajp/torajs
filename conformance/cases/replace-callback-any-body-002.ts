// An arrow's return type is its body's; the context types only its
// parameters. A body that returns `x as any` is an `any` return even
// where the callee's signature expects a string / number / boolean.
console.log("abc".replace(/b/, (m: string) => (m + m) as any));
console.log("abc".replace(/b/g, () => [1, 2] as any));
const xs = [3, 1, 2];
xs.sort((a, b) => (a - b) as any);
console.log(xs.join());
console.log([0, 1, 2, 3].filter((x) => (x % 2) as any).join());
console.log([1, 2].map((x) => String(x) as any).join("|"));
// A user function whose parameter is typed `() => number` reads the
// callback's number: the `as any` body must still hand one back.
function num(f: () => number): number {
  return f() + 1;
}
console.log(num(() => 2 as any));
