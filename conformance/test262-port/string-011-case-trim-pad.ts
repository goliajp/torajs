// Adapted from test262: built-ins/String/prototype/{toUpperCase,toLowerCase,
// trim,trimStart,trimEnd,padStart,padEnd}/* — six common String stdlib
// methods sharing the StrRepr layout. tr's runtime_str.c implements each
// as a single-pass copy: case-fold byte-by-byte (ASCII-only — JS uses
// Unicode units which our v0 doesn't model), trim* skips ASCII whitespace
// at one or both ends, padStart/padEnd repeat-and-truncate the pad string
// to fill the difference up to targetLength.
function check(): number {
  // Case folding (ASCII-only).
  if ("Hello".toUpperCase() !== "HELLO") { throw "#1"; }
  if ("Hello".toLowerCase() !== "hello") { throw "#2"; }
  if ("ABC".toLowerCase() !== "abc") { throw "#3"; }
  if ("123".toUpperCase() !== "123") { throw "#4: digits unchanged"; }
  if ("".toUpperCase() !== "") { throw "#5: empty"; }

  // Trim variants.
  if ("  spaced  ".trim() !== "spaced") { throw "#6"; }
  if ("\t\n hi \r\n".trim() !== "hi") { throw "#7: tab/lf/cr"; }
  if ("  spaced  ".trimStart() !== "spaced  ") { throw "#8"; }
  if ("  spaced  ".trimEnd() !== "  spaced") { throw "#9"; }
  if ("nospace".trim() !== "nospace") { throw "#10"; }
  if ("   ".trim() !== "") { throw "#11: all whitespace"; }

  // padStart / padEnd — basic + already-long-enough cases.
  if ("3".padStart(5, "0") !== "00003") { throw "#12"; }
  if ("42".padStart(4, "ab") !== "ab42") { throw "#13: pad str repeat"; }
  if ("abc".padStart(2, "x") !== "abc") { throw "#14: src already long enough"; }
  if ("3".padEnd(5, ".") !== "3....") { throw "#15"; }
  if ("ab".padEnd(5, "yz") !== "abyzy") { throw "#16: pad str repeat tail"; }
  if ("hello".padEnd(3, "x") !== "hello") { throw "#17: src already long enough"; }

  // Chain — fold then trim.
  let s = "  ABC  ";
  if (s.toLowerCase().trim() !== "abc") { throw "#18"; }
  return 0;
}
console.log(check());
