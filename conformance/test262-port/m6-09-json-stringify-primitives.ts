// Phase M6.3 (partial — primitives only) — JSON.stringify for
// Type::I64 / F64 / Bool / Str. Array / Object / Class-instance
// dispatch is deferred until a recursive walker is wired up.
//
// ssa_lower's Member-call path detects `JSON.stringify(x)` with one
// argument and emits the type-specialized code:
//
//   number  → __torajs_i64_to_str / __torajs_f64_to_str
//   boolean → branch on the value, store interned "true"/"false"
//   string  → __torajs_json_str_quote (wrap with `"` + escape per
//             ECMA-404 §9: "  \\  \n \t \r \b \f  \u00XX)
//
// Three-way agreement (bun + tr-jit + tr-aot) is verified via the
// conformance runner. The escape-coverage line below also serves as
// a sanity check on the runtime quote helper.

function check(): number {
  if (JSON.stringify(0) !== "0") { throw "#1: zero"; }
  if (JSON.stringify(42) !== "42") { throw "#2: positive int"; }
  if (JSON.stringify(-7) !== "-7") { throw "#3: negative int"; }

  if (JSON.stringify(true) !== "true") { throw "#4: bool true"; }
  if (JSON.stringify(false) !== "false") { throw "#5: bool false"; }

  if (JSON.stringify("") !== "\"\"") { throw "#6: empty string"; }
  if (JSON.stringify("hi") !== "\"hi\"") { throw "#7: ascii string"; }
  if (JSON.stringify("a\"b") !== "\"a\\\"b\"") { throw "#8: quote escape"; }
  if (JSON.stringify("a\\b") !== "\"a\\\\b\"") { throw "#9: backslash escape"; }
  if (JSON.stringify("a\nb") !== "\"a\\nb\"") { throw "#10: newline escape"; }
  if (JSON.stringify("a\tb") !== "\"a\\tb\"") { throw "#11: tab escape"; }

  // Mixed via concat — exercise that the result is a real heap str
  // (not just a static slot reference).
  let parts: string[] = [JSON.stringify(1), JSON.stringify(2), JSON.stringify(3)];
  if (parts[0] + "," + parts[1] + "," + parts[2] !== "1,2,3") {
    throw "#12: array of stringified numbers";
  }

  return 0;
}
console.log(check());
