// Adapted from test262: language/expressions/postfix-{increment,decrement}/* —
// JS spec for `x++` and `x--`: yield the OLD value of `x`, THEN mutate.
// Tr previously desugared post-incr as `x = x + 1` (assignment expression
// yielding the new value). Now ssa-lower handles `Expr::PostIncr`
// directly: load → compute new → store new → return the loaded value.
// Three target shapes are covered: Ident, Member, Index.
type Counter = { n: number, _: number };

function check(): number {
  // Ident target.
  let i: number = 5;
  let a = i++;
  if (a !== 5) { throw "#1: post-incr returns old"; }
  if (i !== 6) { throw "#2: post-incr mutates"; }

  let j: number = 10;
  let b = j--;
  if (b !== 10) { throw "#3"; }
  if (j !== 9) { throw "#4"; }

  // Embedded in expression.
  let k: number = 0;
  let s: number = k++ + k++ + k++;  // 0 + 1 + 2 = 3
  if (s !== 3) { throw "#5: chained post-incr"; }
  if (k !== 3) { throw "#6"; }

  // Member target — `obj.field++`.
  let c: Counter = { n: 100, _: 0 };
  let m = c.n++;
  if (m !== 100) { throw "#7: member post-incr returns old"; }
  if (c.n !== 101) { throw "#8: member post-incr mutates"; }

  // Index target — `arr[i]++`.
  let xs: number[] = [10, 20, 30];
  let x0 = xs[1]++;
  if (x0 !== 20) { throw "#9: index post-incr returns old"; }
  if (xs[1] !== 21) { throw "#10: index post-incr mutates"; }

  // Common for-loop pattern still works (result is discarded → no
  // observable difference).
  let acc: number = 0;
  for (let p: number = 0; p < 5; p++) { acc = acc + p; }
  if (acc !== 10) { throw "#11: for-loop post-incr"; }
  return 0;
}
console.log(check());
