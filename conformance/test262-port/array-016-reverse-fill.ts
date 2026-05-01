// Adapted from test262: built-ins/Array/prototype/{reverse,fill}/* — two
// in-place mutators. tr's runtime helpers do the byte-level work over
// the i64-slot layout (8 bytes per element regardless of element type
// — strings/objects/arrays store their pointer there).
//
// Subset constraint: fill on f64-element arrays isn't yet supported
// (would need an IR bitcast to pass the f64 value as i64). Integer,
// boolean, string, and reference-type element arrays all work.
function check(): number {
  // reverse — odd length.
  let xs: number[] = [1, 2, 3, 4, 5];
  xs.reverse();
  if (xs[0] !== 5) { throw "#1"; }
  if (xs[1] !== 4) { throw "#2"; }
  if (xs[2] !== 3) { throw "#3"; }
  if (xs[4] !== 1) { throw "#4"; }

  // reverse — even length.
  let ys: number[] = [10, 20, 30, 40];
  ys.reverse();
  if (ys[0] !== 40) { throw "#5"; }
  if (ys[3] !== 10) { throw "#6"; }

  // reverse — single + empty are no-ops.
  let one: number[] = [42];
  one.reverse();
  if (one[0] !== 42) { throw "#7"; }
  let empty: number[] = [];
  empty.reverse();
  if (empty.length !== 0) { throw "#8"; }

  // String[] reverse.
  let names: string[] = ["a", "b", "c"];
  names.reverse();
  if (names[0] !== "c") { throw "#9"; }
  if (names[2] !== "a") { throw "#10"; }

  // fill — basic, full range.
  let zs: number[] = [0, 0, 0, 0, 0];
  zs.fill(7, 0, 5);
  for (let i: number = 0; i < 5; i = i + 1) {
    if (zs[i] !== 7) { throw "#11: fill all"; }
  }

  // fill — partial range.
  let ws: number[] = [1, 1, 1, 1, 1];
  ws.fill(9, 1, 4);
  if (ws[0] !== 1) { throw "#12"; }
  if (ws[1] !== 9) { throw "#13"; }
  if (ws[3] !== 9) { throw "#14"; }
  if (ws[4] !== 1) { throw "#15"; }

  // fill — clamped end.
  let qs: number[] = [0, 0, 0];
  qs.fill(5, 0, 100);
  if (qs[0] !== 5) { throw "#16: end clamped"; }
  if (qs[2] !== 5) { throw "#17"; }

  // String[] fill.
  let labels: string[] = ["", "", ""];
  labels.fill("x", 0, 3);
  if (labels[0] !== "x") { throw "#18"; }
  if (labels[2] !== "x") { throw "#19"; }
  return 0;
}
console.log(check());
