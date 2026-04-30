// Adapted from test262: language/expressions/strict-equals on strings.
// String identity in tr is by-content (tr's strings are heap-owned but
// `===` compares values).
function check(): number {
  if (("abc" === "abc") !== true) { throw "#1"; }
  if (("abc" === "abd") !== false) { throw "#2"; }
  if (("" === "") !== true) { throw "#3"; }
  let s: string = "hello";
  let t: string = "hello";
  if ((s === t) !== true) { throw "#4"; }
  if ((s !== "world") !== true) { throw "#5"; }
  return 0;
}
console.log(check());
