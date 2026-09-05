// A binding that shadows a top-level `function` inside a BLOCK must be
// read as itself. `{ const t = 3; console.log(t) }` under a top-level
// `function t` whose body says `this` printed `[Function: t]`.
//
// Rotation 594 refused a rebound name in `promote_this_fn_values` and
// its helper's doc carries this exact repro — but that is not the only
// pass that mints the `__forward_` cell at these positions.
// `synthesize_fn_to_closure_forwarders` matches bare Idents against
// the same top-level fn table over a flat arena with no scope
// information, so it wrapped the inner read too. That is why a
// FUNCTION body answered correctly while a bare block, an `if` body
// and every loop body did not.
//
// Only the spellings that could not constant-fold showed it: `t + 1`
// and `const u = t` answered `3` all along, because the fold never
// consults the name table. `console.log(t)` and `${t}` read the
// binding for real, and got the function.

function t(a: any) {
  return "" + a + typeof (this as any);
}

// a bare block
{
  const t = 3;
  console.log(t);
}

// an `if` body
if (true) {
  const t = "s";
  console.log(t);
}

// a loop body
for (let i = 0; i < 1; i++) {
  const t = 7;
  console.log(t);
}

while (true) {
  const t = 8;
  console.log(t);
  break;
}

// `let`, and a nested block
{
  let t = 9;
  console.log(t);
  {
    const t = 10;
    console.log(t);
  }
}

// an initializer that cannot fold at all
function mk(): number {
  return 11;
}
{
  const t = mk();
  console.log(t);
}

// string concatenation, and template interpolation — the two the fold
// used to hide
{
  const t = "ab" + "cd";
  console.log(t, `${t}`);
}

// outside every block the name still means the function
console.log(typeof t, t.name, t.length);

// and a declaration that never says `this` was always right — still is
function q(a: any) {
  return "" + a;
}
{
  const q = 12;
  console.log(q);
}
console.log(typeof q);
