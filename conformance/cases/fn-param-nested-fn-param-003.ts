// A function whose own parameter is fn-typed reaches a higher-order
// function's fn-typed slot as an array element, an object property, a
// ternary arm, an arrow, or through a local alias of a forwarding
// parameter. The higher-order function calls it with an arrow, and
// the function calls what it received.

function run(cb: (x: number) => number): number {
  return cb(3);
}
function hof(f: (g: (x: number) => number) => number): number {
  return f((x) => x * 2);
}
function outerLocal(h: (g: (x: number) => number) => number): number {
  const k = h;
  return hof(k);
}

const fs = [run];
const o = { r: run };
const pick = fs.length > 0;
console.log(hof(fs[0]), hof(o.r), hof(pick ? run : run));
console.log(hof((g) => g(5)), outerLocal(run), outerLocal((g) => g(6)));
