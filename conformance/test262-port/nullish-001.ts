// Adapted from test262: language/expressions/coalesce/* — `??` returns
// rhs only when lhs is exactly null. tr lowers it as alloc-slot +
// pointer null-compare + cond_br: lhs evaluates exactly once, no
// re-evaluation on the non-null path.
type Pt = { x: number, y: number };

function check(): number {
  let p: Pt | null = null;
  let fb: Pt = { x: -1, y: -2 };

  // null lhs → rhs
  let a = p ?? fb;
  if (a.x !== -1) { throw "#1"; }

  // non-null lhs → lhs
  let q: Pt | null = { x: 10, y: 20 };
  let b = q ?? fb;
  if (b.x !== 10) { throw "#2"; }

  // chain: a ?? b ?? c — first non-null wins.
  let n1: Pt | null = null;
  let n2: Pt | null = null;
  let c = n1 ?? n2 ?? fb;
  if (c.x !== -1) { throw "#3"; }

  return 0;
}
console.log(check());
