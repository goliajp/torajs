// Adapted from test262: built-ins/Array/prototype/toSorted/* — the
// non-mutating ES2023 sibling of sort. tr lowers it as `slice(0, len)`
// followed by the existing in-place sort body — single C call to clone
// + identical SSA loop. Source array is untouched after.
function check(): number {
  // Number array — ascending sort.
  let xs: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
  let sorted = xs.toSorted((a: number, b: number): number => a - b);
  if (sorted.length !== 8) { throw "#1: len"; }
  if (sorted[0] !== 1) { throw "#2: head"; }
  if (sorted[1] !== 1) { throw "#3: dup"; }
  if (sorted[7] !== 9) { throw "#4: tail"; }
  // Source untouched.
  if (xs[0] !== 3) { throw "#5: src untouched"; }
  if (xs[7] !== 6) { throw "#6: src untouched tail"; }

  // Descending.
  let desc = xs.toSorted((a: number, b: number): number => b - a);
  if (desc[0] !== 9) { throw "#7: desc head"; }
  if (desc[7] !== 1) { throw "#8: desc tail"; }
  if (xs[0] !== 3) { throw "#9: src still untouched after desc"; }

  // Empty.
  let empty: number[] = [];
  let er = empty.toSorted((a: number, b: number): number => a - b);
  if (er.length !== 0) { throw "#10: empty"; }

  // Single.
  let one: number[] = [42];
  let or_ = one.toSorted((a: number, b: number): number => a - b);
  if (or_.length !== 1) { throw "#11"; }
  if (or_[0] !== 42) { throw "#12"; }

  // Already sorted — output equals input.
  let sorted_in: number[] = [1, 2, 3, 4, 5];
  let so = sorted_in.toSorted((a: number, b: number): number => a - b);
  for (let i: number = 0; i < 5; i = i + 1) {
    if (so[i] !== sorted_in[i]) { throw "#13: identity"; }
  }

  // String array — lex sort via charCodeAt diff.
  let words: string[] = ["banana", "apple", "cherry"];
  let ws = words.toSorted((a: string, b: string): number =>
    a.charCodeAt(0) - b.charCodeAt(0)
  );
  if (ws[0] !== "apple") { throw "#14: str sort"; }
  if (ws[2] !== "cherry") { throw "#15"; }
  if (words[0] !== "banana") { throw "#16: str src untouched"; }

  return 0;
}
console.log(check());
