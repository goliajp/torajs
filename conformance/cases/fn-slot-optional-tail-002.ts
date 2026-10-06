// an optional parameter whose type is an alias of a scalar holds
// `undefined` when the call does not pass it, through every lane: a
// builtin callback with more parameters than the spec passes, a
// callback parameter, a function-typed binding

type Color = number;
type Flag = boolean;
type K = 1 | 2;
[10].forEach((v: number, i: number, a: number[], c?: Color, f?: Flag) =>
  console.log(v, c === undefined, f === undefined, c, f));
function host(cb: (n: number) => string) { return cb(2); }
console.log(host((n, c?: Color, f?: Flag) => [n, c === undefined, f === undefined, c, f].join(" ")));
function w() {
  const g: (n: number) => string = (n, c?: Color) => [n, c === undefined, c].join(" ");
  return g(5);
}
console.log(w());
const e1: (n: number) => string = (n, c?: Color, f?: Flag, k?: K, b?: bigint) =>
  [c === undefined, f === undefined, k === undefined, b === undefined].join(" ");
console.log(e1(1));
function wrap() {
  const z = 1;
  const e2: (n: number) => string = (n, c?: Color, f?: Flag) => [z, c === undefined, f === undefined].join(" ");
  console.log(e2(1));
}
wrap();
function withC(n: number, c?: Color): string { return c === undefined ? "none" : "c" + (c + 1); }
console.log(withC(1), withC(1, 4));
const opt: { c?: Color; f?: Flag } = {};
console.log(opt.c === undefined, opt.f === undefined);
opt.c = 0;
opt.f = false;
console.log(opt.c, opt.f, opt.c === undefined);
