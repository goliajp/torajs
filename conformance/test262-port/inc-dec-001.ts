// Adapted from test262: language/expressions/postfix-increment +
// prefix-increment. Both forms desugar to `x = x + 1` at parse time;
// post-form yields the new value (deviation from spec which yields
// the old value — most uses are in for-loop step where the value is
// discarded, so the deviation is invisible there).
function check(): number {
  let i: number = 5;
  i++;
  if (i !== 6) { throw "#1"; }
  ++i;
  if (i !== 7) { throw "#2"; }
  i--;
  if (i !== 6) { throw "#3"; }
  --i;
  if (i !== 5) { throw "#4"; }

  // Use in for-loop step.
  let sum: number = 0;
  for (let j: number = 0; j < 5; j++) {
    sum += j;
  }
  if (sum !== 10) { throw "#5"; }
  return 0;
}
console.log(check());
