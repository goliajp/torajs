// Adapted from test262: built-ins/Array/prototype/lastIndexOf/* — same
// inline SSA loop as indexOf but the "found" arm Br's back to the
// step block instead of breaking out, so the result_slot ends up
// holding the highest matching index.
function check(): number {
  let xs: number[] = [1, 2, 3, 2, 1];
  if (xs.indexOf(2) !== 1) { throw "#1: first 2"; }
  if (xs.lastIndexOf(2) !== 3) { throw "#2: last 2"; }
  if (xs.indexOf(1) !== 0) { throw "#3"; }
  if (xs.lastIndexOf(1) !== 4) { throw "#4"; }
  if (xs.lastIndexOf(99) !== -1) { throw "#5: miss"; }
  if (xs.lastIndexOf(3) !== 2) { throw "#6: single occurrence"; }

  // String[] case.
  let names: string[] = ["a", "b", "a", "c"];
  if (names.lastIndexOf("a") !== 2) { throw "#7"; }
  if (names.lastIndexOf("b") !== 1) { throw "#8"; }
  if (names.lastIndexOf("z") !== -1) { throw "#9"; }

  // Empty + single.
  let empty: number[] = [];
  if (empty.lastIndexOf(1) !== -1) { throw "#10"; }
  let one: number[] = [42];
  if (one.lastIndexOf(42) !== 0) { throw "#11"; }
  if (one.lastIndexOf(99) !== -1) { throw "#12"; }
  return 0;
}
console.log(check());
