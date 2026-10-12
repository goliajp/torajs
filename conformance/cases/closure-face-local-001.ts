// A function value handed to a fn-typed parameter of a closure that a
// function-local binding holds: the closure cannot be cloned per face,
// so its parameter calls through the boxed entry, which answers each
// value by its own face — the closure is also handed values of the
// parameter's own face.
function mkA(k: number): (y?: number) => any { return (y?: number) => (y === undefined ? 5 : y) + k }

function outer() {
  const inner = (f: (y?: number) => number) => f() + f(1);
  return inner(mkA(3));
}
console.log(outer());

function outer2(n: number) {
  const inner2 = (f: (y?: number) => number) => f() + n;
  return inner2(mkA(5)) + inner2((y?: number) => 1);
}
console.log(outer2(1));

function outer3(n: number) {
  const g: (y?: number) => number = mkA(n);
  const inner3 = (f: (y?: number) => number) => f(2) * 2;
  return [inner3(g), inner3(mkA(n + 1)), inner3((y?: number) => (y === undefined ? 0 : y))].join(",");
}
console.log(outer3(4));
