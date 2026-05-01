// Adapted from test262: language/expressions/function/* — rest
// parameters. tr's `apply_rest_args` AST pass walks every Call site
// whose callee is a fn with a trailing `...rest` param and bundles
// trailing args into an Array literal at the call site. Empty rest
// uses a typed empty-array helper (one per element type) synthesized
// by the same pass.
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

function first_num(...xs: number[]): number {
  return xs[0];
}

function check(): number {
  // Variadic sum.
  if (sum(1, 2, 3) !== 6) { throw "#1"; }
  if (sum(10, 20, 30, 40) !== 100) { throw "#2"; }
  if (sum() !== 0) { throw "#3: empty rest"; }
  if (sum(42) !== 42) { throw "#4: single"; }

  // Required + rest.
  if (tag("x", 1, 2, 3) !== "x: 1 2 3") { throw "#5"; }
  if (tag("y") !== "y:") { throw "#6: empty rest after required"; }
  if (tag("z", 99) !== "z: 99") { throw "#7"; }

  // Concrete-type rest.
  if (first_num(1, 2, 3) !== 1) { throw "#8"; }
  return 0;
}
console.log(check());
