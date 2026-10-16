// A top-level function expression reading `this`, handed to `.call`,
// `.apply`, `map` and `Array.from` inside a named function, beside
// function-local bindings of the same name that are not one (an arrow,
// a number, a parameter). Fractional arguments reach the body whole.
const g = function (this: any, x: number) {
  return typeof this + ":" + x;
};
function run() {
  const o = { k: 1 };
  return [
    g.call(o, 1.5),
    g.call(undefined, 2.25),
    g.apply(o, [3.5]),
    g.apply(undefined, [4.75]),
    [1.5, 2].map(g).join(","),
    Array.from([5.5, 6], g).join(","),
  ].join(" | ");
}
function arrow() {
  const g = (x: number) => x * 2;
  return g(1.5);
}
function num() {
  const g = 7;
  return g + 0.5;
}
function k(g: number) {
  return g + 1;
}
console.log(run());
console.log(arrow(), num(), k(1.5), k(2));
console.log(g.call({}, 7.5), g.apply(null, [8.5]), g(9.5));
function local() {
  const g = function (this: any, x: number) {
    return typeof this + ":" + x;
  };
  const o = { k: 2 };
  return [g.call(o, 1.5), g.apply(o, [2.5]), [3.5].map(g).join(), Array.from([4.5], g).join()].join(" | ");
}
console.log(local());
