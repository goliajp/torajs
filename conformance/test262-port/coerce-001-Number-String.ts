// Adapted from test262: built-ins/Number/* + built-ins/String/* — the
// `Number(x)` / `String(x)` bare-call coercion shapes (JS's
// constructor-without-new). tr's check.rs accepts the call as a
// special form and ssa-lower routes by arg's SSA type.
//
// Subset support:
//   Number(string)  → __torajs_num_parse_float (returns NaN on bad input)
//   Number(number)  → identity
//   Number(boolean) → 1 / 0
//   String(number)  → i64_to_str / f64_to_str
//   String(boolean) → "true" / "false"
//   String(string)  → identity
function check(): number {
  // Number coercion.
  if (Number(42) !== 42) { throw "#1"; }
  if (Number(3.14) !== 3.14) { throw "#2"; }
  if (Number(true) !== 1) { throw "#3"; }
  if (Number(false) !== 0) { throw "#4"; }
  if (Number("42") !== 42) { throw "#5"; }
  if (Number("3.14") !== 3.14) { throw "#6"; }
  if (Number("-7") !== -7) { throw "#7"; }
  if (isNaN(Number("xyz")) !== true) { throw "#8: NaN on bad string"; }

  // String coercion.
  if (String(42) !== "42") { throw "#9"; }
  if (String(0) !== "0") { throw "#10"; }
  if (String(-100) !== "-100") { throw "#11"; }
  if (String(true) !== "true") { throw "#12"; }
  if (String(false) !== "false") { throw "#13"; }
  if (String("hello") !== "hello") { throw "#14"; }

  // Composed: Number(String(x)) round-trip for ints.
  if (Number(String(123)) !== 123) { throw "#15"; }
  return 0;
}
console.log(check());
