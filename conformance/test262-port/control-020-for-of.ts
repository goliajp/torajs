// Adapted from test262: language/statements/for-of/* — for-of over a
// numeric array. tr desugars at parse time into a classic for-loop;
// the source is bound once, length cached, body sees a fresh let
// per iteration.
function check(): number {
  let xs: number[] = [10, 20, 30, 40];
  let sum: number = 0;
  for (let v of xs) {
    sum += v;
  }
  if (sum !== 100) { throw "#1"; }

  // Empty array — body should never run.
  let empty: number[] = [];
  let touched: number = 0;
  for (let v of empty) {
    touched = 999;
  }
  if (touched !== 0) { throw "#2"; }

  // String element type.
  let words: string[] = ["a", "bb", "ccc"];
  let total_len: number = 0;
  for (let w of words) {
    total_len += w.length;
  }
  if (total_len !== 6) { throw "#3"; }

  // break + continue inside for-of.
  let firstOdd: number = -1;
  for (let v of xs) {
    if (v % 2 === 0) { continue; }
    firstOdd = v;
    break;
  }
  // xs is all even → firstOdd stays -1.
  if (firstOdd !== -1) { throw "#4"; }

  return 0;
}
console.log(check());
