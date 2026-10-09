// functions that call each other in a ring, entered from only one of
// them: inlining a callee with one call site into its caller must not
// drop a body the other function still calls
function f(n: number): number {
  return n <= 0 ? 0 : 1 + g(n - 1);
}
function g(n: number): number {
  return n <= 0 ? 0 : 2 + f(n - 1);
}
console.log(f(Number("10")), f(Number("7")));

function p(n: number): number {
  return n <= 0 ? 0 : 1 + q(n - 1);
}
function q(n: number): number {
  return n <= 0 ? 0 : 2 + r(n - 1);
}
function r(n: number): number {
  return n <= 0 ? 0 : 3 + p(n - 1);
}
console.log(p(10));

function even(n: number): boolean {
  if (n === 0) return true;
  const o = odd(n - 1);
  return o;
}
function odd(n: number): boolean {
  if (n === 0) return false;
  const e = even(n - 1);
  return e;
}
console.log(even(Number("10")), even(Number("7")));
