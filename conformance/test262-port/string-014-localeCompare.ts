// Adapted from test262: built-ins/String/prototype/localeCompare/* —
// returns -1 / 0 / 1 based on sort order. Subset uses byte-wise
// memcmp (ASCII-only — JS spec is locale-sensitive but for plain
// ASCII the result is identical).
function check(): number {
  if ("a".localeCompare("b") !== -1) { throw "#1"; }
  if ("b".localeCompare("a") !== 1) { throw "#2"; }
  if ("a".localeCompare("a") !== 0) { throw "#3: equal"; }
  if ("".localeCompare("") !== 0) { throw "#4: both empty"; }
  if ("abc".localeCompare("abd") !== -1) { throw "#5"; }
  if ("abd".localeCompare("abc") !== 1) { throw "#6"; }

  // Length-based when prefix matches.
  if ("ab".localeCompare("abc") !== -1) { throw "#7: shorter"; }
  if ("abc".localeCompare("ab") !== 1) { throw "#8: longer"; }
  if ("".localeCompare("a") !== -1) { throw "#9: empty < non-empty"; }
  if ("a".localeCompare("") !== 1) { throw "#10"; }
  return 0;
}
console.log(check());
