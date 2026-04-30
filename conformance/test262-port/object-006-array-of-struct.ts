// Adapted from test262: array of structured records — verifies the
// element layout for `Type::Obj` inside `Type::Arr`.
type Pt = { x: number, y: number };

function check(): number {
  let pts: Pt[] = [];
  pts.push({ x: 1, y: 2 });
  pts.push({ x: 3, y: 4 });
  pts.push({ x: 5, y: 6 });
  if (pts.length !== 3) { throw "#1"; }
  if (pts[0].x !== 1) { throw "#2"; }
  if (pts[1].y !== 4) { throw "#3"; }
  if (pts[2].x + pts[2].y !== 11) { throw "#4"; }
  return 0;
}
console.log(check());
