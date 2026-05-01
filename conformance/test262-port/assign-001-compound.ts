// Adapted from test262: language/expressions/compound-assignment/*.
// `+=`, `-=`, `*=`, `/=`, `%=` desugar to `x = x op v` at parse time.
// (Note: `/=` produces an f64 result and so we don't combine it with
// an integer slot here — see the f64-slot caveat in collatz/integer
// division.)
function check(): number {
  let n: number = 10;
  n += 5;  if (n !== 15) { throw "#1"; }
  n -= 2;  if (n !== 13) { throw "#2"; }
  n *= 3;  if (n !== 39) { throw "#3"; }
  n %= 7;  if (n !== 4) { throw "#4"; }   // 39 % 7 = 4
  return 0;
}
console.log(check());
