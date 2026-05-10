// V3-18 m1.h.33 — string-literal hex + unicode escapes per JS
// spec §12.8.4.1:
//   \xNN          — 2 hex digits → byte
//   \uNNNN        — 4 hex digits → UTF-16 code unit (encoded UTF-8)
//   \u{N...N}     — 1-6 hex digits → arbitrary code point
//
// Pre-fix lexer's "unknown escape passes through letter" rule
// dropped the `\` and `x` / `u` separately, so `\x41` printed
// "x41" instead of "A". Used pervasively in test262 (regex
// fixtures, Unicode coverage cases, control-char tests).

console.log("\x41\x42\x43")     // ABC
console.log("\x00")              // NUL byte
console.log("A")            // A
console.log("中")            // 中
console.log("é")            // é
console.log("\u{1F600}")         // 😀 (extended hex form)
console.log("\u{2764}")          // ❤
console.log("a\x09b")            // a<TAB>b

// Mixed with regular chars + other escapes (no regression).
console.log("hello\nworld")
console.log("tab\there")
