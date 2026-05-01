// Adapted from test262: built-ins/Array/prototype/{includes,findIndex,some,every}/* —
// short-circuit predicate iteration plus the boolean variant of indexOf.
// All four share the same loop scaffolding: linear scan with early exit.
// `Array.find` is intentionally omitted — it would return `T | undefined`
// which our subset doesn't model for non-pointer T (Nullable<Number>
// would need a tag bit). Use `findIndex(p) >= 0 ? xs[idx] : default`.
function check(): number {
  let xs: number[] = [10, 20, 30, 40, 50];

  if (xs.includes(30) !== true) { throw "#1"; }
  if (xs.includes(35) !== false) { throw "#2"; }
  if (xs.includes(10) !== true) { throw "#3: first elem"; }
  if (xs.includes(50) !== true) { throw "#4: last elem"; }

  if (xs.findIndex((v: number): boolean => v === 30) !== 2) { throw "#5"; }
  if (xs.findIndex((v: number): boolean => v > 100) !== -1) { throw "#6: miss"; }
  if (xs.findIndex((v: number): boolean => v >= 40) !== 3) { throw "#7"; }

  if (xs.some((v: number): boolean => v > 25) !== true) { throw "#8"; }
  if (xs.some((v: number): boolean => v > 100) !== false) { throw "#9: empty match"; }
  if (xs.every((v: number): boolean => v > 5) !== true) { throw "#10"; }
  if (xs.every((v: number): boolean => v > 25) !== false) { throw "#11: not all"; }
  if (xs.every((v: number): boolean => v > 0) !== true) { throw "#12"; }

  // String[] case — exercises the Type::Str comparison path.
  let names: string[] = ["alpha", "beta", "gamma"];
  if (names.includes("beta") !== true) { throw "#13"; }
  if (names.includes("delta") !== false) { throw "#14"; }
  if (names.findIndex((s: string): boolean => s === "gamma") !== 2) { throw "#15"; }

  // Empty-array edge cases.
  let empty: number[] = [];
  if (empty.some((v: number): boolean => true) !== false) { throw "#16: some on []"; }
  if (empty.every((v: number): boolean => false) !== true) { throw "#17: every on [] is true"; }
  if (empty.findIndex((v: number): boolean => true) !== -1) { throw "#18"; }
  return 0;
}
console.log(check());
