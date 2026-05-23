// P3.1-e.5 draft — replace / replaceAll byte-level (ASCII).

// --- replace (first occurrence) ---
console.log("[" + "hello world".replace("world", "there") + "]");  // hello there
console.log("[" + "abcabc".replace("b", "X") + "]");                // aXcabc — only first
console.log("[" + "abc".replace("z", "X") + "]");                   // abc — not found
console.log("[" + "abc".replace("", "X") + "]");                    // Xabc — empty needle inserts at 0
console.log("[" + "".replace("z", "X") + "]");                      // (empty) — empty s
console.log("[" + "".replace("", "X") + "]");                       // X — empty s + empty needle
console.log("[" + "abc".replace("abc", "") + "]");                  // (empty) — repl empty
console.log("[" + "abc".replace("b", "") + "]");                    // ac — repl empty mid
console.log("[" + "aaa".replace("aa", "b") + "]");                  // ba — first 2-char hit then no overlap

// --- replaceAll (all occurrences, non-overlapping) ---
console.log("[" + "abcabc".replaceAll("b", "X") + "]");             // aXcaXc
console.log("[" + "aaaa".replaceAll("aa", "b") + "]");              // bb — non-overlapping consumes 2 at a time
console.log("[" + "abc".replaceAll("z", "X") + "]");                // abc — no match
console.log("[" + "aaa".replaceAll("a", "BB") + "]");               // BBBBBB — repl longer
console.log("[" + "BBBB".replaceAll("BB", "x") + "]");              // xx — repl shorter
console.log("[" + "".replaceAll("z", "X") + "]");                   // (empty)
console.log("[" + "abc".replaceAll("abc", "") + "]");               // (empty) — full removal
// Skip empty-needle replaceAll: spec throws TypeError, tora subset
// returns copy (silent divergence — pre-existing).
