// Adapted from test262: language/literals/null/* + nullable type
// annotations. tr only allows nullable for pointer-shaped T (Str /
// Obj / Arr / Closure / FnSig); `number | null` is not in the subset.
type Pt = { x: number, y: number };

function check(): number {
  let p: Pt | null = null;
  if ((p === null) !== true) { throw "#1"; }
  if ((p !== null) !== false) { throw "#2"; }

  let q: Pt | null = { x: 5, y: 7 };
  if ((q === null) !== false) { throw "#3"; }

  // null-on-null compare.
  let r: Pt | null = null;
  if ((r === p) !== true) { throw "#4"; }

  // String nullable.
  let s: string | null = null;
  if ((s === null) !== true) { throw "#5"; }
  s = "hi";
  if ((s === null) !== false) { throw "#6"; }
  return 0;
}
console.log(check());
