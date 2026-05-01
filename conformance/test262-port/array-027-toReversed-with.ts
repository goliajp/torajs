// Adapted from test262: built-ins/Array/prototype/{toReversed,with}/* —
// the non-mutating ES2023 siblings of reverse / xs[i] = v. Both are
// fresh-alloc operations: source is untouched, caller gets a brand
// new array.
function check(): number {
  // toReversed — number array.
  let xs: number[] = [1, 2, 3, 4, 5];
  let ys = xs.toReversed();
  if (ys.length !== 5) { throw "#1: len"; }
  if (ys[0] !== 5) { throw "#2: head"; }
  if (ys[4] !== 1) { throw "#3: tail"; }
  // Source untouched.
  if (xs[0] !== 1) { throw "#4: src untouched head"; }
  if (xs[4] !== 5) { throw "#5: src untouched tail"; }

  // toReversed — empty.
  let empty: number[] = [];
  let er = empty.toReversed();
  if (er.length !== 0) { throw "#6: empty"; }

  // toReversed — single element.
  let one: number[] = [42];
  let or_ = one.toReversed();
  if (or_.length !== 1) { throw "#7"; }
  if (or_[0] !== 42) { throw "#8"; }

  // toReversed — string array.
  let words: string[] = ["alpha", "beta", "gamma"];
  let wr = words.toReversed();
  if (wr[0] !== "gamma") { throw "#9: str head"; }
  if (wr[2] !== "alpha") { throw "#10: str tail"; }
  if (words[0] !== "alpha") { throw "#11: src str untouched"; }

  // with — number array, positive index.
  let zs: number[] = [10, 20, 30, 40];
  let z2 = zs.with(2, 99);
  if (z2.length !== 4) { throw "#12: len"; }
  if (z2[0] !== 10) { throw "#13"; }
  if (z2[1] !== 20) { throw "#14"; }
  if (z2[2] !== 99) { throw "#15: replaced"; }
  if (z2[3] !== 40) { throw "#16"; }
  // Source untouched.
  if (zs[2] !== 30) { throw "#17: src untouched"; }

  // with — head replacement.
  let z3 = zs.with(0, 7);
  if (z3[0] !== 7) { throw "#18: with head"; }
  if (z3[3] !== 40) { throw "#19"; }

  // with — tail replacement.
  let z4 = zs.with(3, 99);
  if (z4[3] !== 99) { throw "#20: with tail"; }
  if (z4[0] !== 10) { throw "#21"; }

  // with — negative index wraps.
  let z5 = zs.with(-1, 88);
  if (z5[3] !== 88) { throw "#22: with -1"; }
  let z6 = zs.with(-4, 77);
  if (z6[0] !== 77) { throw "#23: with -len"; }

  // with — string elements.
  let strs: string[] = ["a", "b", "c"];
  let s2 = strs.with(1, "X");
  if (s2[0] !== "a") { throw "#24"; }
  if (s2[1] !== "X") { throw "#25"; }
  if (s2[2] !== "c") { throw "#26"; }
  if (strs[1] !== "b") { throw "#27: src str untouched"; }

  // Pipe: toReversed -> with.
  let pipe = xs.toReversed().with(0, 100);
  if (pipe[0] !== 100) { throw "#28: pipe head"; }
  if (pipe[1] !== 4) { throw "#29: pipe rest"; }

  return 0;
}
console.log(check());
