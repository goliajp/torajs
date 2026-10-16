// One function declares `g` as a function expression reading `this`
// and calls it bare (strict mode: `this` is undefined); other
// functions and an arrow take a parameter named `g` — a number, a
// callback handed to `map` and called, a string, and a parameter that
// a `var g` in the body re-assigns.

function a(): string {
  const g = function (this: any) {
    return typeof this;
  };
  return g();
}
function b(g: number): number {
  return g + 1;
}
function c(g: (x: number) => number): string {
  return [1, 2].map(g).join(",") + " " + g(5);
}
const k = (g: string) => g.length;
function d(g: any): number {
  var g = 4;
  return g;
}
console.log(a(), b(2), c((x) => x * 10), k("abc"), d(1));
