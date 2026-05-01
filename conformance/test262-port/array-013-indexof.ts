// Adapted from test262: built-ins/Array/prototype/indexOf/* —
// `arr.indexOf(needle)` linear scan; returns -1 on miss. tr emits an
// inline SSA loop; per-iter compare picks ICmp / FCmp / __torajs_str_eq
// based on the array's element type. No allocations, LLVM
// auto-vectorizes the i64 case.
function check(): number {
  let xs: number[] = [10, 20, 30, 20, 40];
  if (xs.indexOf(30) !== 2) { throw "#1"; }
  if (xs.indexOf(20) !== 1) { throw "#2: first match"; }
  if (xs.indexOf(99) !== -1) { throw "#3: miss"; }
  if (xs.indexOf(10) !== 0) { throw "#4: head"; }
  if (xs.indexOf(40) !== 4) { throw "#5: tail"; }

  // Empty array.
  let empty: number[] = [];
  if (empty.indexOf(0) !== -1) { throw "#6: empty"; }

  // String elements use __torajs_str_eq for content compare.
  let words: string[] = ["alpha", "beta", "gamma", "beta"];
  if (words.indexOf("beta") !== 1) { throw "#7"; }
  if (words.indexOf("zeta") !== -1) { throw "#8"; }
  if (words.indexOf("alpha") !== 0) { throw "#9"; }
  return 0;
}
console.log(check());
