// P5.5 — static `f(...[a, b, c])` literal-array spread folded
// into `f(a, b, c)` at parser time, so fixed-arity functions
// accept the spread without runtime arity surgery. Dynamic
// spread (`f(...someVar)`) still requires a rest-param sig.

function sum3(a: number, b: number, c: number): number {
  return a + b + c;
}
console.log(sum3(...[1, 2, 3]));    // 6
console.log(sum3(...[10, 20, 30])); // 60

function add(a: number, b: number, c: number, d: number): number {
  return a + b + c + d;
}
// Mixed literal + spread: positionals around the spread expand.
console.log(add(1, ...[2, 3], 4));  // 10

function greet(prefix: string, name: string): string {
  return prefix + " " + name;
}
console.log(greet(...["Hello", "World"]));  // Hello World
