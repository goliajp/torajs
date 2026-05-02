// Adapted from test262: built-ins/Array/prototype/flatMap. tr's
// subset constrains the callback to `(T) => T[]` (homogeneous), and
// returns `T[]` (one-level flatten). Inner arrays of any length are
// supported (including length 0 / 1 / N).
function check(): number {
  // Number flatMap — duplicate each element.
  let xs: number[] = [1, 2, 3];
  let dups: number[] = xs.flatMap((n: number): number[] => [n, n * 10]);
  if (dups.length !== 6) { throw "#1: len"; }
  if (dups[0] !== 1) { throw "#2"; }
  if (dups[1] !== 10) { throw "#3"; }
  if (dups[2] !== 2) { throw "#4"; }
  if (dups[3] !== 20) { throw "#5"; }
  if (dups[4] !== 3) { throw "#6"; }
  if (dups[5] !== 30) { throw "#7"; }

  // Variable inner length — odd elements yield 0, evens yield 2.
  let ys: number[] = [1, 2, 3, 4];
  let mix: number[] = ys.flatMap((n: number): number[] => {
    if (n % 2 === 0) { return [n, n + 100]; }
    let empty: number[] = [];
    return empty;
  });
  if (mix.length !== 4) { throw "#8: len"; }
  if (mix[0] !== 2) { throw "#9"; }
  if (mix[1] !== 102) { throw "#10"; }
  if (mix[2] !== 4) { throw "#11"; }
  if (mix[3] !== 104) { throw "#12"; }

  // String flatMap — duplicate each element. Refcounted path.
  let strs: string[] = ["ab", "cd"];
  let dup_strs: string[] = strs.flatMap((s: string): string[] => [s, s]);
  if (dup_strs.length !== 4) { throw "#13"; }
  if (dup_strs[0] !== "ab") { throw "#14"; }
  if (dup_strs[1] !== "ab") { throw "#15"; }
  if (dup_strs[2] !== "cd") { throw "#16"; }
  if (dup_strs[3] !== "cd") { throw "#17"; }

  // Empty source.
  let empty: number[] = [];
  let r: number[] = empty.flatMap((n: number): number[] => [n, n]);
  if (r.length !== 0) { throw "#18: empty"; }

  // Closure body produces Array<Substr> (split result), declared as
  // Array<Str>. Return-coerce materializes each substr to owned Str
  // so flatMap sees Array<Str> as expected.
  let ws: string[] = ["ab", "cd"];
  let chars: string[] = ws.flatMap((s: string): string[] => s.split(""));
  if (chars.length !== 4) { throw "#19: chars.len " + chars.length; }
  if (chars[0] !== "a") { throw "#20"; }
  if (chars[1] !== "b") { throw "#21"; }
  if (chars[2] !== "c") { throw "#22"; }
  if (chars[3] !== "d") { throw "#23"; }
  return 0;
}
console.log(check());
