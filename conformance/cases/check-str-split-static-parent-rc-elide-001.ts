// P14-S9 — `__torajs_str_split` hoists FLAG_STATIC_LITERAL check on
// the parent string once and elides per-substr `__torajs_rc_inc(parent)`
// calls in the static-parent fast path. The bench-only-100k fixture
// (literal `"3 4 + 2 * 5 +"`-split) hits this path 100k times.
//
// Correctness verification — three orthogonal scenarios that must all
// stay byte-equal with bun:
//
// 1. Static parent + multi-token split (the bench shape) — exercises
//    the static-parent fast path, the trailing-token branch, and the
//    inline-substr drop walker that releases parent without re-incing.
// 2. Dynamic parent (concat result) + multi-token split — exercises
//    the dynamic-parent path that still calls rc_inc on every substr.
//    Parent's refcount must stay balanced across the inline-substr
//    lifetime so the dynamic string is freed exactly once.
// 3. Static parent + per-char split (`split("")`) — exercises the
//    per-char arm of the same static-parent fast path.

// (1) static parent, multi-token
let s1 = "3 4 + 2 * 5 +";
let total1: number = 0;
for (let i: number = 0; i < 100; i = i + 1) {
  let parts: string[] = s1.split(" ");
  total1 = total1 + parts.length;
}
console.log(total1);
let parts1: string[] = s1.split(" ");
console.log(parts1.length);
console.log(parts1[0]);
console.log(parts1[6]);

// (2) dynamic parent (concat) — parent is NOT static, rc_inc still required
let s2: string = "a b" + " c d e";
let parts2: string[] = s2.split(" ");
console.log(parts2.length);
console.log(parts2[0]);
console.log(parts2[4]);
// Drop s2 by going out of scope — parent rc balance must be intact.

// (3) static parent, per-char split (empty separator)
let parts3: string[] = "abc".split("");
console.log(parts3.length);
console.log(parts3[0]);
console.log(parts3[2]);

// (4) static parent, no-match (single trailing token only)
let parts4: string[] = "no separator here".split("|");
console.log(parts4.length);
console.log(parts4[0]);
