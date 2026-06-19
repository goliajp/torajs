// ES §22.1.3.21 step 6 — `s.split(sep, limit)` reads `limit`
// through ToUint32, which truncates toward 0 for finite numbers
// and yields 0 for NaN / ±0. Pre-fix tr panicked the codegen
// backend with "GPR materialization can't hold an f64" because
// the inline `ICmp(Slt, limit_op, len)` and `Store(limit_op,
// slot)` chain consumed the raw f64 literal directly. Same root
// cause family as Array.at non-integer (commit 41ceb22e).

console.log("a,b,c,d".split(",", 1.5));  // [ "a" ]       (trunc 1.5 → 1)
console.log("a,b,c,d".split(",", 2.9));  // [ "a", "b" ]  (trunc 2.9 → 2)
console.log("a,b,c,d".split(",", 0.5));  // []
console.log("a,b,c,d".split(",", NaN));   // []           (NaN → 0)
console.log("a,b,c,d".split(",", 4));    // [ "a","b","c","d" ]   (limit ≥ len)
console.log("a,b,c,d".split(",", 0));    // []
console.log("a,b,c,d".split(",", 3));    // [ "a", "b", "c" ]
