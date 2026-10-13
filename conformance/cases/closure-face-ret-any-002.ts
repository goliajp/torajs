// A function declared to return a number that hands back what an
// `any`-returning callback answers keeps that value as is (§10.2.1.12)
// when it flows on as a value: into a binding annotated with a function
// type, and into a fn-typed parameter of another function.
function q(x: number, f: () => any): number {
  return f();
}
const h: (x: number, f: () => any) => number = q;
console.log(h(1, () => "s"));
function run(g: (x: number, f: () => any) => number): number {
  return g(2, () => "t");
}
console.log(run(q));
console.log(typeof run(q), q(3, () => 4) + 1);
function twice(x: number, f: () => any): number {
  return f() + f();
}
console.log(run(twice), run((x: number, f: () => any): number => x * 10));
