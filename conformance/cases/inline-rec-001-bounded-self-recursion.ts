// Phase 2.1 inliner: bounded self-recursion inline (depth 1) +
// multi-ret stack-slot join + nested-call clone-through. fib has two
// self-call sites and two value-bearing returns; min2 has two
// value-bearing returns joined through the slot path; wrap calls a
// non-leaf callee.
function fib(n: number): number {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

function min2(a: number, b: number): number {
  if (a < b) return a;
  return b;
}

function wrap(a: number, b: number): number {
  return min2(a, b) + min2(b, a);
}

console.log(fib(20));
console.log(min2(7, 3));
console.log(min2(2, 9));
console.log(wrap(5, 8));
