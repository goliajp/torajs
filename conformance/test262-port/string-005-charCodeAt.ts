// Adapted from test262: built-ins/String/prototype/charCodeAt/*.js
// Returns the UTF-16 code unit at the given index. We exercise ASCII only
// (the runtime's string layout is bytes; high-bit chars are out of scope).
function check(): number {
  if ("hello".charCodeAt(0) !== 104) { throw "#1: h"; }
  if ("hello".charCodeAt(1) !== 101) { throw "#2: e"; }
  if ("hello".charCodeAt(4) !== 111) { throw "#3: o"; }
  if ("AB".charCodeAt(0) !== 65) { throw "#4: A"; }
  if ("AB".charCodeAt(1) !== 66) { throw "#5: B"; }
  return 0;
}
console.log(check());
