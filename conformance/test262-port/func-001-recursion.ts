// Adapted from test262: language/statements/function/* + factorial style.
// Recursive function returns; verifies stack frames + return value plumbing.
function fact(n: number): number {
  if (n <= 1) { return 1; }
  return n * fact(n - 1);
}

function check(): number {
  if (fact(0) !== 1) { throw "#1: fact(0)"; }
  if (fact(1) !== 1) { throw "#2: fact(1)"; }
  if (fact(5) !== 120) { throw "#3: fact(5)"; }
  if (fact(10) !== 3628800) { throw "#4: fact(10)"; }
  return 0;
}
console.log(check());
