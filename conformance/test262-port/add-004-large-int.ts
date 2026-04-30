// Adapted from test262: language/expressions/addition/* — large-magnitude
// integer adds. tr's `number` is i64; values fit comfortably below 2^53.
function check(): number {
  if (1000000 + 1000000 !== 2000000) { throw "#1"; }
  if (-500 + 500 !== 0) { throw "#2"; }
  if (-1000000 + 999999 !== -1) { throw "#3"; }
  let big: number = 1000000000;
  if (big + big !== 2000000000) { throw "#4"; }
  return 0;
}
console.log(check());
