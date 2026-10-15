// A function compared with `==` / `!=` against null or undefined: a
// function object is never nullish (§7.2.14), whether it is a declared
// function, a function expression, an arrow or a callback parameter.
function f(x: number) {
  return x + 1;
}
const e = function (s: string) {
  return s + "!";
};
const k = 3;
const a = (x: number) => x * k;
function run(cb: (x: number) => number) {
  return [cb == null, cb != null, undefined == cb, cb != undefined].join(",");
}
console.log(f == null, f != null, null == f, f != undefined, undefined == f);
console.log(e == null, e != undefined, a == null, a != null);
console.log(run(f), run(a));
console.log(f === null, f !== undefined, a === undefined);
let g: ((x: number) => number) | null = null;
console.log(g == null, g != null);
g = f;
console.log(g == null, g != null);
