// Adapted from test262: built-ins/Array/prototype/findLastIndex/* —
// the reverse-iteration sibling of findIndex (ES2023). Returns -1 on
// miss like findIndex, so it sidesteps the `T | undefined` problem
// that keeps `find` / `findLast` out of the subset. tr lowers it as
// the same predicate-loop scaffolding with `i = len-1; i >= 0; i -= 1`.
function check(): number {
  let xs: number[] = [10, 20, 30, 20, 40];

  // Last matching index — distinct from first.
  if (xs.findLastIndex((v: number): boolean => v === 20) !== 3) {
    throw "#1: dup match — last wins";
  }
  if (xs.findIndex((v: number): boolean => v === 20) !== 1) {
    throw "#2: forward unchanged";
  }

  // Last matching index — single occurrence.
  if (xs.findLastIndex((v: number): boolean => v === 30) !== 2) { throw "#3"; }
  if (xs.findLastIndex((v: number): boolean => v === 10) !== 0) { throw "#4: head"; }
  if (xs.findLastIndex((v: number): boolean => v === 40) !== 4) { throw "#5: tail"; }

  // Miss returns -1 (no Nullable<Number> needed).
  if (xs.findLastIndex((v: number): boolean => v > 1000) !== -1) { throw "#6: miss"; }
  if (xs.findLastIndex((v: number): boolean => v < 0) !== -1) { throw "#7: miss neg"; }

  // Empty array.
  let empty: number[] = [];
  if (empty.findLastIndex((v: number): boolean => v === 0) !== -1) { throw "#8: empty"; }

  // Single-element array — same answer either direction.
  let one: number[] = [42];
  if (one.findLastIndex((v: number): boolean => v === 42) !== 0) { throw "#9"; }
  if (one.findLastIndex((v: number): boolean => v !== 42) !== -1) { throw "#10"; }

  // String elements.
  let words: string[] = ["alpha", "beta", "gamma", "beta"];
  if (words.findLastIndex((s: string): boolean => s === "beta") !== 3) {
    throw "#11: string last";
  }
  if (words.findLastIndex((s: string): boolean => s === "alpha") !== 0) {
    throw "#12: head";
  }
  if (words.findLastIndex((s: string): boolean => s === "delta") !== -1) {
    throw "#13: miss";
  }

  // Predicate uses comparison operators.
  let nums: number[] = [5, 15, 25, 35, 45];
  if (nums.findLastIndex((v: number): boolean => v < 30) !== 2) { throw "#14: <"; }
  if (nums.findLastIndex((v: number): boolean => v > 30) !== 4) { throw "#15: >"; }

  return 0;
}
console.log(check());
