// Adapted from test262: built-ins/String/prototype/endsWith/*.js
function check(): number {
  if ("hello world".endsWith("world") !== true) { throw "#1"; }
  if ("hello world".endsWith("hello") !== false) { throw "#2"; }
  if ("hello world".endsWith("") !== true) { throw "#3"; }
  if ("abc".endsWith("abc") !== true) { throw "#4"; }
  if ("abc".endsWith("xabc") !== false) { throw "#5"; }
  return 0;
}
console.log(check());
