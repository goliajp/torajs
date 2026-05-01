// Adapted from test262: built-ins/global/{parseInt,parseFloat,isNaN,isFinite}/* —
// the bare-identifier JS globals (no Number prefix). tr routes them to
// the same Number.X intrinsics; the spec's coercion behavior of
// global isNaN / isFinite (coerce non-number args first) doesn't fire
// here since the subset's typecheck rejects non-Number args directly.
function check(): number {
  // Bare-name parseInt / parseFloat.
  if (parseInt("42", 10) !== 42) { throw "#1"; }
  if (parseInt("ff", 16) !== 255) { throw "#2"; }
  if (parseInt("0x2a", 16) !== 42) { throw "#3: 0x prefix"; }
  if (parseInt("3.14", 10) !== 3) { throw "#4: stops at ."; }
  if (parseFloat("3.14") !== 3.14) { throw "#5"; }
  if (parseFloat("-2.5") !== -2.5) { throw "#6"; }

  // Bare-name isNaN / isFinite.
  if (isNaN(0) !== false) { throw "#7"; }
  if (isNaN(42) !== false) { throw "#8"; }
  if (isNaN(parseInt("xyz", 10)) !== true) { throw "#9: parseInt failure"; }
  if (isFinite(0) !== true) { throw "#10"; }
  if (isFinite(parseInt("zzz", 10)) !== false) { throw "#11"; }
  if (isFinite(Number.POSITIVE_INFINITY) !== false) { throw "#12"; }

  // Default radix == 10 (when arg omitted from the bare-name form).
  if (parseInt("100") !== 100) { throw "#13: default radix 10"; }
  return 0;
}
console.log(check());
