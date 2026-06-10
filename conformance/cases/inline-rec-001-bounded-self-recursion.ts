// Phase 2.1 inliner: non-leaf inlining + multi-ret stack-slot join.
// fib's body (two self-calls, two value-bearing returns) inlines into
// main as a non-leaf multi-ret callee — the cloned self-calls stay as
// calls (self-recursion itself is not inlined at the default depth 0);
// min2 exercises the slot join; wrap calls a non-leaf callee.
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
