// Adapted from test262: built-ins/Number/{parseInt,parseFloat,isInteger,
// isNaN,isFinite}/* — five Number stdlib statics. tr's runtime helpers
// live in runtime_str.c; ssa-lower routes calls based on arg type
// (i64 vs f64 picks the cheaper variant; i64 args are trivially
// integer / non-NaN / finite).
//
// Subset constraint: Number.parseInt radix must be an integer-typed
// expression (literal `10` or i64 binding). f64-typed radix isn't
// supported in v0.
function check(): number {
  // parseInt — decimal
  if (Number.parseInt("42", 10) !== 42) { throw "#1"; }
  if (Number.parseInt("-7", 10) !== -7) { throw "#2: negative"; }
  if (Number.parseInt("  9  ", 10) !== 9) { throw "#3: leading ws"; }

  // parseInt — hex
  if (Number.parseInt("ff", 16) !== 255) { throw "#4"; }
  if (Number.parseInt("0x2a", 16) !== 42) { throw "#5: 0x prefix"; }
  if (Number.parseInt("ABC", 16) !== 2748) { throw "#6: uppercase hex"; }

  // parseInt — binary
  if (Number.parseInt("1011", 2) !== 11) { throw "#7"; }

  // parseInt — stops at first non-digit
  if (Number.parseInt("42abc", 10) !== 42) { throw "#8"; }
  if (Number.parseInt("3.14", 10) !== 3) { throw "#9: stops at ."; }

  // parseFloat
  if (Number.parseFloat("3.14") !== 3.14) { throw "#10"; }
  if (Number.parseFloat("-2.5") !== -2.5) { throw "#11: negative"; }
  if (Number.parseFloat("1e3") !== 1000) { throw "#12: scientific"; }
  if (Number.parseFloat("42") !== 42) { throw "#13: integer-only"; }

  // isInteger
  if (Number.isInteger(7) !== true) { throw "#14"; }
  if (Number.isInteger(3.5) !== false) { throw "#15: fractional"; }
  if (Number.isInteger(0) !== true) { throw "#16: zero"; }
  if (Number.isInteger(-100) !== true) { throw "#17: negative integer"; }
  if (Number.isInteger(2.0) !== true) { throw "#18: 2.0 is integer-valued"; }

  // isNaN — strict (no coercion)
  if (Number.isNaN(0) !== false) { throw "#19"; }
  if (Number.isNaN(42) !== false) { throw "#20"; }
  if (Number.isNaN(Number.parseInt("abc", 10)) !== true) { throw "#21: parseInt failure"; }
  if (Number.isNaN(Number.parseFloat("xyz")) !== true) { throw "#22: parseFloat failure"; }

  // isFinite
  if (Number.isFinite(0) !== true) { throw "#23"; }
  if (Number.isFinite(3.14) !== true) { throw "#24"; }
  if (Number.isFinite(Number.parseInt("zzz", 10)) !== false) { throw "#25: NaN not finite"; }
  return 0;
}
console.log(check());
