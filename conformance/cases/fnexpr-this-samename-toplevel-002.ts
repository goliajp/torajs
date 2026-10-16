// A top-level function expression reading `this` next to a same-name
// one in a function: the top-level call passes its number unboxed, and
// a fractional argument reaches either body whole.
const g = function (this: any, x: number) {
  return "top-" + typeof this + ":" + x;
};
console.log(g(1), g(2.5));
function h() {
  const g = function (this: any, x: number) {
    return "in-" + typeof this + ":" + x;
  };
  return g(2) + " " + g(3.75);
}
console.log(h());
