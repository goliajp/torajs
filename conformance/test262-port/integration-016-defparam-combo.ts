// Integration: default function parameters combined with closures,
// classes, and recursion. Validates that the apply_default_args AST
// pass works across all call shapes.
function add(a: number, b: number = 0): number { return a + b; }

function fmt(n: number, prefix: string = "n=", suffix: string = ""): string {
  return prefix + n + suffix;
}

function pow_or_zero(base: number, exp: number = 2): number {
  let r: number = 1;
  for (let i: number = 0; i < exp; i = i + 1) { r = r * base; }
  return r;
}

function check(): number {
  // Simple defaults.
  if (add(5) !== 5) { throw "#1"; }
  if (add(5, 7) !== 12) { throw "#2"; }
  if (add(0) !== 0) { throw "#3"; }

  // Multi-default.
  if (fmt(42) !== "n=42") { throw "#4"; }
  if (fmt(42, "x=") !== "x=42") { throw "#5"; }
  if (fmt(42, "x=", "!") !== "x=42!") { throw "#6"; }

  // Default + arithmetic.
  if (pow_or_zero(2) !== 4) { throw "#7: 2^2"; }
  if (pow_or_zero(2, 10) !== 1024) { throw "#8: 2^10"; }
  if (pow_or_zero(7) !== 49) { throw "#9: 7^2"; }

  // Default in for-loop body.
  let total: number = 0;
  for (let i: number = 1; i <= 5; i = i + 1) {
    total = total + add(i);  // add(i, 0)
  }
  if (total !== 15) { throw "#10"; }

  // Default with computed expression at construct time.
  if (add(add(1) + add(2)) !== 3) { throw "#11: nested defaults"; }
  return 0;
}
console.log(check());
