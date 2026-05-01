// Adapted from test262: language/expressions/function/* — function
// parameter defaults. tr's `apply_default_args` AST pass walks every
// `Expr::Call` with an Ident callee and pads `args` with the callee's
// default ExprIds when trailing args are omitted.
//
// Subset constraint: defaults are evaluated at the call site (not in
// callee scope) — slightly diverges from JS spec but covers typical
// constant / global-expression defaults.
function add(a: number, b: number = 10): number {
  return a + b;
}

function greet(name: string = "world"): string {
  return "hi " + name;
}

function bounds(lo: number = 0, hi: number = 100): number {
  return hi - lo;
}

function check(): number {
  // Single default — caller omits last arg.
  if (add(5) !== 15) { throw "#1"; }
  if (add(5, 7) !== 12) { throw "#2: explicit overrides default"; }
  if (add(0) !== 10) { throw "#3"; }
  if (add(-3) !== 7) { throw "#4"; }

  // String default.
  if (greet() !== "hi world") { throw "#5"; }
  if (greet("alice") !== "hi alice") { throw "#6"; }

  // Multiple defaults.
  if (bounds() !== 100) { throw "#7"; }
  if (bounds(5) !== 95) { throw "#8: only first default"; }
  if (bounds(5, 50) !== 45) { throw "#9"; }
  return 0;
}
console.log(check());
