// Adapted from test262: built-ins/Array/prototype/slice/* —
// `arr.slice(start, end)` returns a fresh array of the [start, end)
// range. tr's runtime helper does a single bounds-clamp + memcpy;
// element-type-agnostic (every slot is 8 bytes, so number / string /
// struct / closure all share the same path).
function check(): number {
  let xs: number[] = [10, 20, 30, 40, 50];

  let mid = xs.slice(1, 4);
  if (mid.length !== 3) { throw "#1"; }
  if (mid[0] !== 20) { throw "#2"; }
  if (mid[2] !== 40) { throw "#3"; }

  // Whole-range clamp.
  let head = xs.slice(0, 100);
  if (head.length !== 5) { throw "#4"; }
  if (head[0] !== 10) { throw "#5"; }
  if (head[4] !== 50) { throw "#6"; }

  // Inverted range yields empty.
  let empty = xs.slice(3, 1);
  if (empty.length !== 0) { throw "#7"; }

  // Slice of strings.
  let words: string[] = ["a", "bb", "ccc", "dddd"];
  let tail = words.slice(2, 4);
  if (tail.length !== 2) { throw "#8"; }
  if (tail[0] !== "ccc") { throw "#9"; }
  if (tail[1] !== "dddd") { throw "#10"; }

  return 0;
}
console.log(check());
