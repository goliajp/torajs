// Adapted from test262: TS-shape — same struct passed to multiple
// functions, then mutated, then re-passed. Was rejected by tr's old
// affine consume rule; landed once function args switched to
// borrow-by-default semantics.
type P = { x: number, y: number };

function getX(p: P): number { return p.x; }
function getY(p: P): number { return p.y; }
function sum(p: P): number { return p.x + p.y; }

function check(): number {
  let p: P = { x: 3, y: 4 };
  if (getX(p) !== 3) { throw "#1"; }
  if (getY(p) !== 4) { throw "#2"; }
  if (sum(p) !== 7) { throw "#3"; }
  // mutate then re-pass
  p.x = 100;
  if (sum(p) !== 104) { throw "#4"; }
  return 0;
}
console.log(check());
