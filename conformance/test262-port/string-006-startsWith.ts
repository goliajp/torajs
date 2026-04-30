// Adapted from test262: built-ins/String/prototype/startsWith/*.js
function check(): number {
  if ("hello world".startsWith("hello") !== true) { throw "#1"; }
  if ("hello world".startsWith("world") !== false) { throw "#2"; }
  if ("hello world".startsWith("") !== true) { throw "#3"; }
  if ("abc".startsWith("abc") !== true) { throw "#4"; }
  if ("abc".startsWith("abcd") !== false) { throw "#5"; }
  return 0;
}
console.log(check());
