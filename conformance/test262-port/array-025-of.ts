// Adapted from test262: built-ins/Array/of/* — `Array.of(...vals)` is
// the variadic factory ES6 introduced to disambiguate `Array(7)` (length
// 7) from `Array.of(7)` (single-element array). tr emits the same SSA
// as a no-spread array literal so the cost is identical to writing
// `[a, b, c]` by hand.
function check(): number {
  // Single-arg — disambiguates from new Array(n) length-allocation.
  let one = Array.of(7);
  if (one.length !== 1) { throw "#1: length"; }
  if (one[0] !== 7) { throw "#2"; }

  // Multi-arg, number elements.
  let xs = Array.of(1, 2, 3, 4, 5);
  if (xs.length !== 5) { throw "#3: len5"; }
  if (xs[0] !== 1) { throw "#4"; }
  if (xs[4] !== 5) { throw "#5"; }

  // Sum-pipe through reduce.
  let sum = Array.of(10, 20, 30).reduce((a: number, x: number): number => a + x, 0);
  if (sum !== 60) { throw "#6: reduce"; }

  // String elements.
  let words = Array.of("alpha", "beta", "gamma");
  if (words.length !== 3) { throw "#7: words"; }
  if (words[1] !== "beta") { throw "#8"; }
  if (words.join("-") !== "alpha-beta-gamma") { throw "#9: join"; }

  // Boolean elements.
  let flags = Array.of(true, false, true);
  if (flags.length !== 3) { throw "#10"; }
  if (flags[0] !== true) { throw "#11"; }
  if (flags[1] !== false) { throw "#12"; }

  // Mixed pipe with map.
  let doubled = Array.of(2, 4, 6).map((n: number): number => n * 10);
  if (doubled[0] !== 20) { throw "#13"; }
  if (doubled[2] !== 60) { throw "#14"; }

  return 0;
}
console.log(check());
