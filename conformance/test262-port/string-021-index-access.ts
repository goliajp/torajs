// Adapted from test262: built-ins/String/prototype/charAt-aliased.
// `s[i]` is the index-access form of `s.charAt(i)` — returns a
// single-char string. tr lowers it to a substr view of length 1
// referencing the source's bytes, so it costs one 32-byte view alloc
// and no byte copy. Empty / OOB indices are unchecked (matches the
// subset's index-access convention used elsewhere).
function check(): number {
  let s = "hello";
  if (s[0] !== "h") { throw "#1"; }
  if (s[1] !== "e") { throw "#2"; }
  if (s[2] !== "l") { throw "#3"; }
  if (s[3] !== "l") { throw "#4"; }
  if (s[4] !== "o") { throw "#5"; }

  // Indexing a substring view (split result) — view-of-view that
  // collapses to root parent.
  let parts: string[] = "abc,defg".split(",");
  if (parts[0][0] !== "a") { throw "#6"; }
  if (parts[0][2] !== "c") { throw "#7"; }
  if (parts[1][0] !== "d") { throw "#8"; }
  if (parts[1][3] !== "g") { throw "#9"; }

  // Single-char source.
  let one = "z";
  if (one[0] !== "z") { throw "#10"; }
  return 0;
}
console.log(check());
