// Two functions each declare their own `g`. In `a` it is a function
// expression reading `this`, called bare (strict mode: `this` is
// undefined); in the others it is a number, an arrow handed to array
// methods and called, and a function re-bound with `.call`. Each `g`
// is its own binding and keeps its own behaviour.

function a(): string {
  const g = function (this: any) {
    return this === undefined ? "undef" : typeof this;
  };
  return g();
}
function b(): number {
  const g = 5;
  return g + 1;
}
function c(): string {
  const g = (x: number) => x * 2;
  const xs = [1, 2, 3].map(g);
  return xs.join(",") + " " + g(10) + " " + [3, 1, 2].sort((p, q) => g(p) - g(q)).join("");
}
function d(): number {
  const g = function (this: { k: number } | undefined, x: number) {
    return (this === undefined ? 0 : this.k) + x;
  };
  return g.call({ k: 7 }, 1) + g(2);
}
function e(): string {
  const g = "str";
  return g.toUpperCase();
}
console.log(a(), b(), c(), d(), e());
