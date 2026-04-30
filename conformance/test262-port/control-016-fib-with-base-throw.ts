// Combo test: recursive function that uses throw for the base case
// (instead of return). Exercises throw + recursion + caller catch.
function fib(n: number): number {
  if (n < 0) { throw "#negative"; }
  if (n < 2) { return n; }
  return fib(n - 1) + fib(n - 2);
}

function check(): number {
  if (fib(0) !== 0) { throw "#1"; }
  if (fib(1) !== 1) { throw "#2"; }
  if (fib(10) !== 55) { throw "#3"; }
  let caught: string = "";
  try {
    fib(-1);
  } catch (e: string) {
    caught = e;
  }
  if (caught !== "#negative") { throw "#4"; }
  return 0;
}
console.log(check());
