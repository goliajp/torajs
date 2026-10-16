// A top-level function expression reading `this`, called at top level
// and from a function, beside function-local bindings of the same name
// (another `this`-reading function, a plain arrow, a number, a class
// method's local). Its arguments keep their types, fractions included.
const g = function (this: any, x: number, y: string) {
  return "top-" + typeof this + ":" + x + y;
};
console.log(g(1, "a"), g(1.25, "f"));
console.log(g.call({}, 2, "b"), g.call(undefined, 2.5, "c"));
function h() {
  const g = function (this: any, x: number, y: string) {
    return "in-" + typeof this + ":" + x + y;
  };
  return g(7, "h") + " " + top();
}
function top() {
  return g(8.5, "t");
}
console.log(h());
function plain() {
  const g = (x: number) => "arrow:" + x;
  return g(3.5);
}
function num() {
  const g = 5;
  return g + 1;
}
class K {
  run() {
    const g = (s: string) => s + "!";
    return g("k");
  }
}
console.log(plain(), num(), new K().run());
function local() {
  const r = function (this: any, x: number) {
    return typeof this + ":" + x;
  };
  return r(1.5);
}
console.log(local());
