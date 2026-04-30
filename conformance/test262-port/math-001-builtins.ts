// Adapted from test262: built-ins/Math/{abs,min,max,floor,ceil,sqrt}/*.js
// Single-arg + two-arg method coverage. Constants checked against literal
// double-precision values.
function check(): number {
  if (Math.abs(-5) !== 5) { throw "#1"; }
  if (Math.abs(7) !== 7) { throw "#2"; }
  if (Math.min(3, 4) !== 3) { throw "#3"; }
  if (Math.max(3, 4) !== 4) { throw "#4"; }
  if (Math.floor(3.7) !== 3) { throw "#5"; }
  if (Math.ceil(3.2) !== 4) { throw "#6"; }
  if (Math.sqrt(16) !== 4) { throw "#7"; }
  if (Math.pow(2, 10) !== 1024) { throw "#8"; }
  return 0;
}
console.log(check());
