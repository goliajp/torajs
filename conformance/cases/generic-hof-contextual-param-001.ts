// An arrow passed to a generic function takes its parameter types from
// the parameter's function type even when the type parameter appears
// only in the return; the type argument is then inferred from the body.
function run<T>(f: (n: number) => T): T {
  return f(5);
}
function hof<T>(f: (g: (x: number) => number) => T): T {
  return f((x) => x * 2);
}
function make<T>(f: (a: number, b: string) => T): T {
  return f(2, "x");
}
function dbl(x: number): number {
  return x * 2;
}
function hofNamed<T>(f: (g: (x: number) => number) => T): T {
  return f(dbl);
}

console.log(run((n) => n * 2));
console.log(run((n) => "v" + n));
console.log(hof((g) => g(5)));
console.log(hof<number>((g) => g(6)));
console.log(hof((g) => "s" + g(7)));
console.log(make((a, b) => b + a));
console.log(hofNamed((g) => g(8)));
