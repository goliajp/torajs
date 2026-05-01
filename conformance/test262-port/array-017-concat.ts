// Adapted from test262: built-ins/Array/prototype/concat/* — fresh array
// of receiver's elements followed by the argument's. Subset is binary
// only (`a.concat(b)`); JS allows multi-arg `a.concat(b, c, d)` and
// even non-array args (which get appended as single elements). Both
// arrays must share element type for the static-shape SSA to type-
// check.
function check(): number {
  let a: number[] = [1, 2, 3];
  let b: number[] = [4, 5, 6];
  let c = a.concat(b);
  if (c.length !== 6) { throw "#1"; }
  if (c[0] !== 1) { throw "#2"; }
  if (c[2] !== 3) { throw "#3"; }
  if (c[3] !== 4) { throw "#4"; }
  if (c[5] !== 6) { throw "#5"; }

  // Empty + non-empty.
  let empty: number[] = [];
  let one: number[] = [42];
  let r1 = empty.concat(one);
  if (r1.length !== 1) { throw "#6"; }
  if (r1[0] !== 42) { throw "#7"; }
  let r2 = one.concat(empty);
  if (r2.length !== 1) { throw "#8"; }
  if (r2[0] !== 42) { throw "#9"; }

  // Empty + empty.
  let r3 = empty.concat(empty);
  if (r3.length !== 0) { throw "#10"; }

  // Source unchanged (concat is non-destructive).
  if (a.length !== 3) { throw "#11: a unchanged"; }
  if (b.length !== 3) { throw "#12: b unchanged"; }

  // String[] concat.
  let s1: string[] = ["alpha", "beta"];
  let s2: string[] = ["gamma", "delta"];
  let s3 = s1.concat(s2);
  if (s3.length !== 4) { throw "#13"; }
  if (s3[0] !== "alpha") { throw "#14"; }
  if (s3[3] !== "delta") { throw "#15"; }
  return 0;
}
console.log(check());
