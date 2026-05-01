// Adapted from test262: language/expressions/call/* — spread in call
// args. Subset supports the common shape: `f(req…, ...arr)` where f
// has a trailing rest param. The `apply_rest_args` AST pass passes
// the spread source array directly as the rest param (no allocation).
function sum(...nums: number[]): number {
  let s: number = 0;
  for (let n of nums) { s = s + n; }
  return s;
}

function tag(name: string, ...vals: number[]): string {
  let r = name + ":";
  for (let v of vals) { r = r + " " + v; }
  return r;
}

function check(): number {
  // Basic spread.
  let xs: number[] = [1, 2, 3, 4, 5];
  if (sum(...xs) !== 15) { throw "#1"; }

  // Spread of empty array.
  let empty: number[] = [];
  if (sum(...empty) !== 0) { throw "#2"; }

  // Required + spread.
  let vs: number[] = [10, 20, 30];
  if (tag("x", ...vs) !== "x: 10 20 30") { throw "#3"; }

  // Spread + post-evaluation.
  let big = sum(...[100, 200, 300]);
  if (big !== 600) { throw "#4: spread of array literal"; }

  // No-spread (regular variadic) still works.
  if (sum(7, 8, 9) !== 24) { throw "#5"; }
  return 0;
}
console.log(check());
