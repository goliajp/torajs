// Adapted from test262: built-ins/String/length/S15.5.5.1_A1.js
// torajs: string.length is in bytes, not UTF-16 code units (caveat).
function check(): number {
  if ("".length !== 0) { throw "#1"; }
  if ("a".length !== 1) { throw "#2"; }
  if ("abc".length !== 3) { throw "#3"; }
  if ("hello world".length !== 11) { throw "#4"; }
  return 0;
}
console.log(check());
