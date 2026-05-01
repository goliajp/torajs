// Adapted from test262: language/expressions/assignment/destructuring-array.
// Array destructuring desugars at parse time to per-element index reads;
// source bound once.
function check(): number {
  let xs: number[] = [10, 20, 30, 40];
  let [a, b, c] = xs;
  if (a !== 10) { throw "#1"; }
  if (b !== 20) { throw "#2"; }
  if (c !== 30) { throw "#3"; }

  // Source is a non-Ident expression — should still evaluate once.
  let [m, n] = [100, 200];
  if (m !== 100) { throw "#4"; }
  if (n !== 200) { throw "#5"; }
  return 0;
}
console.log(check());
