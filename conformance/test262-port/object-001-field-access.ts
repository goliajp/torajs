// Adapted from test262: language/expressions/property-accessors/*.js
// Direct field read on an object literal — JS's `obj.x` ↔ tr's
// `type T = { x: number }` + `let o: T = { x: ... }`.
type Point = { x: number, y: number };

function check(): number {
  let p: Point = { x: 3, y: 4 };
  if (p.x !== 3) { throw "#1: p.x"; }
  if (p.y !== 4) { throw "#2: p.y"; }
  if (p.x + p.y !== 7) { throw "#3: p.x + p.y"; }
  return 0;
}
console.log(check());
