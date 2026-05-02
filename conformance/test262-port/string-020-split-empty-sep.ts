// Adapted from test262: built-ins/String/prototype/split/separator-
// is-empty-string. TS spec: `s.split("")` returns an array of single-
// character substrings. Earlier tr returned `[whole-s]` which is the
// regex / undefined-separator behavior — wrong for the empty-string
// separator case.
function check(): number {
  let s = "abcd";
  let chars: string[] = s.split("");
  if (chars.length !== 4) { throw "#1: len " + chars.length; }
  if (chars[0] !== "a") { throw "#2"; }
  if (chars[1] !== "b") { throw "#3"; }
  if (chars[2] !== "c") { throw "#4"; }
  if (chars[3] !== "d") { throw "#5"; }

  // Empty source.
  let empty = "";
  let zero: string[] = empty.split("");
  if (zero.length !== 0) { throw "#6: empty " + zero.length; }

  // Single char source.
  let one = "x";
  let single: string[] = one.split("");
  if (single.length !== 1) { throw "#7"; }
  if (single[0] !== "x") { throw "#8"; }
  return 0;
}
console.log(check());
