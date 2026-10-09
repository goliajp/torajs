// A function whose own parameter is fn-typed reaches a higher-order
// function's bare fn-typed slot through a const alias, a let alias or
// a forwarding parameter rather than by name. The higher-order
// function calls it with an arrow, and the function calls what it
// received. Nothing here hands the slot itself a closure.

function run(cb: (x: number) => number): number {
  return cb(3);
}
function hof(f: (g: (x: number) => number) => number): number {
  return f((x) => x * 2);
}
function outer(h: (g: (x: number) => number) => number): number {
  return hof(h);
}

const r = run;
console.log(hof(r), outer(r), outer(run));

function main(): void {
  const m = run;
  console.log(hof(m), outer(m));
}
main();

// a plain alias of a function handed to a bare fn-typed parameter
function dbl(x: number): number {
  return x * 2;
}
function app(f: (x: number) => number): number {
  return f(2);
}
const d = dbl;
let e = dbl;
console.log(app(d), app(e), d(4), e(5));
