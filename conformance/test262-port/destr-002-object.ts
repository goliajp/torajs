// Adapted from test262: language/expressions/assignment/destructuring-object.
// Object destructuring desugars to per-field member reads; rename via
// `field: bound` syntax.
type Pt = { x: number, y: number };

function check(): number {
  let p: Pt = { x: 3, y: 4 };
  let { x, y } = p;
  if (x !== 3) { throw "#1"; }
  if (y !== 4) { throw "#2"; }

  // Renaming
  let { x: foo, y: bar } = p;
  if (foo !== 3) { throw "#3"; }
  if (bar !== 4) { throw "#4"; }

  // Inline object literal as source.
  let { x: a, y: b } = { x: 7, y: 9 };
  if (a !== 7) { throw "#5"; }
  if (b !== 9) { throw "#6"; }
  return 0;
}
console.log(check());
