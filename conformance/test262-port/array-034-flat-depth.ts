// Adapted from test262: built-ins/Array/prototype/flat. tr's subset
// requires the depth arg to be a number literal so the type checker
// can statically peel that many Array<> layers from the receiver's
// element type. depth=0 is a shallow clone (returns Array<T_0>);
// depth>0 unrolls into N calls to the depth-1 runtime helper.
function check(): number {
  // Default depth = 1.
  let xs: number[][] = [[1, 2], [3, 4], [5, 6]];
  let f1: number[] = xs.flat();
  if (f1.length !== 6) { throw "#1: len"; }
  if (f1[0] !== 1) { throw "#2"; }
  if (f1[5] !== 6) { throw "#3"; }

  // Explicit depth = 1 (same as default).
  let f1b: number[] = xs.flat(1);
  if (f1b.length !== 6) { throw "#4"; }
  if (f1b[3] !== 4) { throw "#5"; }

  // Depth = 2 — peel two Array<> layers.
  let ys: number[][][] = [[[1, 2], [3]], [[4], [5, 6]]];
  let f2: number[] = ys.flat(2);
  if (f2.length !== 6) { throw "#6: f2.len"; }
  if (f2[0] !== 1) { throw "#7"; }
  if (f2[5] !== 6) { throw "#8"; }

  // Depth = 0 — shallow clone. Result has the same shape.
  let zs: number[] = [10, 20, 30];
  let f0: number[] = zs.flat(0);
  if (f0.length !== 3) { throw "#9"; }
  if (f0[0] !== 10) { throw "#10"; }

  // String-array nested flatten.
  let ss: string[][] = [["a", "b"], ["c"], ["d", "e"]];
  let fs: string[] = ss.flat();
  if (fs.length !== 5) { throw "#11"; }
  if (fs[0] !== "a") { throw "#12"; }
  if (fs[4] !== "e") { throw "#13"; }
  return 0;
}
console.log(check());
