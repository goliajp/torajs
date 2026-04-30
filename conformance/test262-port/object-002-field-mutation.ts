// Adapted from test262: language/expressions/assignment/property-target.js
// In-place field write via `obj.x = v`.
type Counter = { value: number };

function check(): number {
  let c: Counter = { value: 0 };
  c.value = 10;
  if (c.value !== 10) { throw "#1"; }
  c.value = c.value + 5;
  if (c.value !== 15) { throw "#2"; }
  c.value = -c.value;
  if (c.value !== -15) { throw "#3"; }
  return 0;
}
console.log(check());
