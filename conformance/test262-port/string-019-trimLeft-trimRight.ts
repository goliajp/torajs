// Adapted from test262: built-ins/String/prototype/{trimLeft,trimRight}/* —
// the Annex-B legacy aliases for trimStart / trimEnd. Every modern JS
// engine ships them; tr routes both to the same str_trim_start /
// str_trim_end intrinsics so behavior is identical to the new spelling.
function check(): number {
  // trimLeft / trimRight basic.
  if ("  hello".trimLeft() !== "hello") { throw "#1: trimLeft"; }
  if ("hello  ".trimRight() !== "hello") { throw "#2: trimRight"; }

  // Mixed: only one side affected.
  if ("  ab  ".trimLeft() !== "ab  ") { throw "#3: trailing kept"; }
  if ("  ab  ".trimRight() !== "  ab") { throw "#4: leading kept"; }

  // Tabs and newlines treated as whitespace too.
  if ("\t\n  hi".trimLeft() !== "hi") { throw "#5: tab+newline"; }
  if ("hi\t\n  ".trimRight() !== "hi") { throw "#6: trailing tab+newline"; }

  // No-op cases.
  if ("clean".trimLeft() !== "clean") { throw "#7: no leading"; }
  if ("clean".trimRight() !== "clean") { throw "#8: no trailing"; }

  // All-whitespace yields empty.
  if ("   ".trimLeft() !== "") { throw "#9: all ws left"; }
  if ("   ".trimRight() !== "") { throw "#10: all ws right"; }

  // Identity vs canonical names.
  let s = "  spaced  ";
  if (s.trimLeft() !== s.trimStart()) { throw "#11: alias trimStart"; }
  if (s.trimRight() !== s.trimEnd()) { throw "#12: alias trimEnd"; }

  return 0;
}
console.log(check());
