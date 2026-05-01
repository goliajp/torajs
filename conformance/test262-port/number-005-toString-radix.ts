// Adapted from test262: built-ins/Number/prototype/toString/* —
// `n.toString(radix)`. Radix in [2, 36] selects the digit alphabet;
// negative numbers get a leading `-`. tr's runtime uses a stack
// buffer + downward-fill algorithm. Subset is i64-receiver only;
// f64 toString stays decimal-only via the existing f64_to_str path.
function check(): number {
  // Hex.
  if ((255).toString(16) !== "ff") { throw "#1"; }
  if ((256).toString(16) !== "100") { throw "#2"; }
  if ((0).toString(16) !== "0") { throw "#3"; }
  if ((-15).toString(16) !== "-f") { throw "#4: negative"; }

  // Binary.
  if ((8).toString(2) !== "1000") { throw "#5"; }
  if ((255).toString(2) !== "11111111") { throw "#6"; }
  if ((0).toString(2) !== "0") { throw "#7"; }
  if ((1).toString(2) !== "1") { throw "#8"; }

  // Octal.
  if ((8).toString(8) !== "10") { throw "#9"; }
  if ((63).toString(8) !== "77") { throw "#10"; }

  // Decimal (default).
  if ((42).toString() !== "42") { throw "#11"; }
  if ((100).toString(10) !== "100") { throw "#12"; }
  if ((-100).toString() !== "-100") { throw "#13"; }

  // Higher radixes — base 36 alphanumeric.
  if ((35).toString(36) !== "z") { throw "#14"; }
  if ((36).toString(36) !== "10") { throw "#15"; }

  // Round-trip via parseInt.
  let s = (1234).toString(16);
  if (parseInt(s, 16) !== 1234) { throw "#16: round-trip"; }
  return 0;
}
console.log(check());
