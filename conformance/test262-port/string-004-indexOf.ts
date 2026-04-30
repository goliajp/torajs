// Adapted from test262: built-ins/String/prototype/indexOf/*.js
function check(): number {
  if ("hello world".indexOf("hello") !== 0) { throw "#1"; }
  if ("hello world".indexOf("world") !== 6) { throw "#2"; }
  if ("hello world".indexOf("o") !== 4) { throw "#3"; }
  if ("hello world".indexOf("xyz") !== -1) { throw "#4"; }
  if ("".indexOf("") !== 0) { throw "#5"; }
  if ("abc".indexOf("") !== 0) { throw "#6"; }
  return 0;
}
console.log(check());
