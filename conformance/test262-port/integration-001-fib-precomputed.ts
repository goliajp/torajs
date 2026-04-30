// Integration: precomputed fibonacci table built outside the closure,
// then a closure capturing the table provides O(1) lookup. (Pushing to
// a captured array from inside the closure is currently broken — the
// closure body's local slot updates after `push` realloc, but the env
// block keeps the pre-realloc pointer; covered separately by the
// "captured-array push" caveat in docs.)
function makeLookup(): (n: number) => number {
  let table: number[] = [0, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377];
  return (n: number): number => table[n];
}

function check(): number {
  let fib = makeLookup();
  if (fib(0) !== 0) { throw "#1"; }
  if (fib(1) !== 1) { throw "#2"; }
  if (fib(10) !== 55) { throw "#3"; }
  if (fib(14) !== 377) { throw "#4"; }
  return 0;
}
console.log(check());
