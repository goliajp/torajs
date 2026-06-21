// V0.2 P14-S6 — `str_split` single-byte-needle SIMD fast path.
// Latin-1 haystack split on a 1-byte separator (the dominant
// shapes `" "`, `","`, `"\n"`, `";"`) now goes through a tight
// byte-equality scan in both `count_matches` and the per-substr
// emit loop. LLVM auto-vectorizes the scan to NEON
// `pcmpeq + popcount` on ARM64. Multi-byte separators and UTF-16
// haystacks keep the original slice-equality scan path.
// Bench: split-only-100k tr 7.59 → ~5.8 ms (post-S6 hyperfine
// 1.51x faster than bun on mini, meeting the framing's per-case
// ≥ 1.5x target on this case for the first time).

// (1) Single-space separator (bench shape):
console.log("3 4 + 2 * 5 +".split(" ").length);
console.log("3 4 + 2 * 5 +".split(" ").join("|"));

// (2) Comma separator — single byte:
console.log("a,b,c,d,e".split(","));
console.log("".split(","));
console.log("only".split(","));

// (3) Trailing separator (empty trailing token):
console.log("a,b,".split(",").length); // 3 (last is "")

// (4) Leading separator (empty leading token):
console.log(",a,b".split(",").length); // 3 (first is "")

// (5) Consecutive separators (empty middle tokens):
console.log("a,,b".split(",").length); // 3 (middle is "")

// (6) Multi-byte separator — falls back to generic path:
console.log("a::b::c".split("::"));

// (7) Per-char split (sep = "") — separate code path, unaffected:
console.log("abc".split(""));

// (8) Separator longer than haystack — single trailing token:
console.log("ab".split("longer"));

// (9) Hot loop (verify no internal state leaks):
let total: number = 0;
for (let i: number = 0; i < 50; i = i + 1) {
  total = total + "x,y,z,a,b,c".split(",").length;
}
console.log(total);
