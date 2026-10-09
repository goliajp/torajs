// A function handed to a fn-typed parameter whose own parameter is
// fn-typed: the higher-order function calls it with an arrow, a named
// function, a capturing arrow or its own fn-typed parameter, and the
// passed function calls what it received.

function run(cb: (x: number) => number): number {
  return cb(3);
}
function twice(x: number): number {
  return x * 2;
}
function half(x: number): number {
  return x / 2;
}

function viaArrow(f: (g: (x: number) => number) => number): number {
  return f((x) => x * 2);
}
function viaNamed(f: (g: (x: number) => number) => number): number {
  return f(twice);
}
function viaCapture(f: (g: (x: number) => number) => number, k: number): number {
  return f((x) => x * k);
}
function viaParam(f: (g: (x: number) => number) => number, g2: (x: number) => number): number {
  return f(g2);
}
function viaFloat(f: (g: (x: number) => number) => number): number {
  return f((x) => x / 2);
}

console.log(viaArrow(run), viaNamed(run), viaCapture(run, 7));
console.log(viaParam(run, twice), viaParam(run, half), viaFloat(run));

// the same function also called directly, with a named function and an arrow
console.log(run(twice), run((x) => x + 1));

// a closure-let caller and a closure-let callee
const lam = (f: (g: (x: number) => number) => number): number => f((x) => x + 1);
const four = (h: (x: number) => number): number => h(4);
console.log(lam(run), viaArrow(four), lam(four));
