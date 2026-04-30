// Adapted from test262: built-ins/String/prototype/slice/*.js
// Pulls subset where bounds are in-range and not negative-from-end (we don't
// support negative start = "from end" yet).
function check(): number {
  if ("hello".slice(0, 5) !== "hello") { throw "#1"; }
  if ("hello".slice(0, 3) !== "hel") { throw "#2"; }
  if ("hello".slice(2, 5) !== "llo") { throw "#3"; }
  if ("hello".slice(0, 0) !== "") { throw "#4"; }
  if ("hello".slice(5, 5) !== "") { throw "#5"; }
  return 0;
}
console.log(check());
