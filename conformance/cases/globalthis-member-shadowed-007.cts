// A class declared in one function's block is not in scope in
// another function, whose `globalThis.Math` reads the builtin.
function other() {
  {
    class Math {
      static max(a, b) {
        return 7;
      }
    }
  }
  return 0;
}
function reader() {
  return globalThis.Math.max(1, 2);
}
console.log(other(), reader());
const viaArrow = () => {
  {
    class Reflect {
      static has() {
        return "mine";
      }
    }
  }
  return 0;
};
console.log(viaArrow(), globalThis.Reflect.has({ a: 1 }, "a"));
