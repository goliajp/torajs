// A same-name arrow inside a generic function and a generic class
// method: each instantiation copies the body, and the copied `g`
// still names the arrow, not the `this`-reading function expression
// declared in `a`.

function a(): string {
  const g = function (this: any, x: number) {
    return (this === undefined ? "u" : "o") + x;
  };
  return g(1) + g.call({}, 2);
}
function h<T>(v: T): string {
  const g = (x: number) => x + 1;
  return String(v) + g(2) + [1].map(g).join("");
}
class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  m(): string {
    const g = (x: number) => x * 10;
    return String(this.v) + g(3);
  }
}
console.log(a(), h(4), h("s"), new Box(1).m(), new Box("b").m());
