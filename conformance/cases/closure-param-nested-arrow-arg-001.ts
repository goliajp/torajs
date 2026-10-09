// An arrow handed to a parameter whose function type takes a function:
// the callee calls it with closures and with named functions alike.
function dbl(x: number): number {
  return x * 2;
}
function hof(f: (g: (x: number) => number) => number): number {
  return f((x) => x * 2);
}
function hofNamed(f: (g: (x: number) => number) => number): number {
  return f(dbl);
}
function hofLet(f: (g: (x: number) => number) => number): number {
  const c = (x: number) => x + 100;
  return f(c);
}
function twice(f: (g: (x: number) => number, v: number) => number): number {
  return f((x) => x - 1, 10) + f(dbl, 10);
}

console.log(hof((g) => g(5)));
console.log(hof((g: (x: number) => number) => g(6)));
console.log(hofNamed((g) => g(7)));
console.log(hofLet((g) => g(8)));
console.log(twice((g, v) => g(v) * 3));
const base = 1000;
console.log(hof((g) => g(9) + base));
