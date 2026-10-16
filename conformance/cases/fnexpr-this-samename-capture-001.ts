// A top-level `g` and function-local `g`s: the top-level one is an
// arrow read from a top-level function, from arrows and from nested
// arrows; one local `g` is a function expression reading `this`,
// called bare, through a captured arrow and as a `map` callback
// (strict mode: `this` is undefined each time).

const g = (x: number) => x * 3;
const k = () => g(1) + [2].map(g)[0];
function top(): string {
  return [1, 2].map(g).join(",") + ":" + g(5) + ":" + g.call(null, 6);
}
function nested(): number {
  const outer = () => {
    const inner = () => g(4);
    return inner();
  };
  return outer();
}
function a(): string {
  const g = function (this: any, x: number) {
    return (this === undefined ? "u" : "o") + x;
  };
  const h = () => g(0);
  return [1, 2].map(g).join(",") + " " + h() + " " + g.call({}, 4);
}
function b(): number {
  const g = (x: number) => x + 1;
  const run = () => [5, 6].map(g).reduce((p, q) => p + q, 0) + g(2);
  return run();
}
console.log(k(), top(), nested(), a(), b());
