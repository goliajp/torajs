// Adapted from test262: built-ins/String/prototype/includes/*.js
// Drops cases requiring 2-arg includes(searchString, fromIndex) — we
// only support 1-arg.
function check(): number {
  if ("hello world".includes("hello") !== true) { throw "#1"; }
  if ("hello world".includes("world") !== true) { throw "#2"; }
  if ("hello world".includes("xyz") !== false) { throw "#3"; }
  if ("".includes("") !== true) { throw "#4"; }
  if ("a".includes("") !== true) { throw "#5"; }
  return 0;
}
console.log(check());
