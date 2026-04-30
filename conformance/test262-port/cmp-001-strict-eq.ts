// Adapted from test262: language/expressions/strict-equals — number cases.
// Drops Number wrapper / NaN / +0/-0 / undefined / null cases (out of subset).
function check(): number {
  if ((1 === 1) !== true) { throw "#1"; }
  if ((1 === 2) !== false) { throw "#2"; }
  if (("a" === "a") !== true) { throw "#3"; }
  if (("a" === "b") !== false) { throw "#4"; }
  if ((true === true) !== true) { throw "#5"; }
  if ((false === false) !== true) { throw "#6"; }
  if ((true === false) !== false) { throw "#7"; }
  return 0;
}
console.log(check());
