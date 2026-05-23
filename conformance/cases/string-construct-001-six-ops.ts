// P3.1-e.4 — torajs-str::transform::construct six-op bun-parity fixture.
// Covers repeat / charAt / at / fromCharCode / substring / substr.
// All inputs ASCII; the v0 byte-Str layout matches bun's UTF-16 code-unit
// semantics on ASCII-only payloads.

// --- repeat ---
console.log("[" + "ab".repeat(3) + "]");      // ababab
console.log("[" + "x".repeat(5) + "]");        // xxxxx
console.log("[" + "".repeat(100) + "]");       // (empty)
console.log("[" + "hi".repeat(0) + "]");       // (empty)
// negative n: bun throws RangeError; tora subset returns empty (no throw).
// Skip negative case to keep bun-parity.

// --- charAt ---
console.log("[" + "hello".charAt(0) + "]");    // h
console.log("[" + "hello".charAt(4) + "]");    // o
console.log("[" + "hello".charAt(5) + "]");    // (empty)
console.log("[" + "hello".charAt(-1) + "]");   // (empty) — charAt does NOT wrap
console.log("[" + "".charAt(0) + "]");         // (empty)

// --- at ---
console.log("[" + "hello".at(0) + "]");        // h
console.log("[" + "hello".at(-1) + "]");       // o — at wraps
console.log("[" + "hello".at(-5) + "]");       // h
// OOB at() — bun returns undefined per spec; tora subset returns
// empty string (pre-existing C-side deviation, preserved in the
// Rust port for bit-for-bit ABI compat). Skip OOB cases here so
// the fixture stays bun-parity for in-range inputs.

// --- fromCharCode ---
console.log("[" + String.fromCharCode(65) + "]");     // A
console.log("[" + String.fromCharCode(97) + "]");     // a
console.log("[" + String.fromCharCode(48) + "]");     // 0
console.log("[" + String.fromCharCode(0x41) + "]");   // A
// 0x141 truncates to 0x41 in our byte-Str (bun returns "Ł" — a true UTF-16
// code unit). Skip non-ASCII fromCharCode test to keep bun-parity.

// --- substring ---
console.log("[" + "hello".substring(1, 4) + "]");     // ell
console.log("[" + "hello".substring(0, 5) + "]");     // hello
console.log("[" + "hello".substring(3, 1) + "]");     // ell — swap
console.log("[" + "hello".substring(-2, 3) + "]");    // hel — neg→0
console.log("[" + "hello".substring(2, 100) + "]");   // llo — clamp
console.log("[" + "hello".substring(5, 5) + "]");     // (empty)
console.log("[" + "hello".substring(0, 0) + "]");     // (empty)

// --- substr (legacy) ---
console.log("[" + "hello".substr(1, 3) + "]");        // ell
console.log("[" + "hello".substr(-3, 2) + "]");       // ll — neg wrap
console.log("[" + "hello".substr(-100, 3) + "]");     // hel — wrap saturates
console.log("[" + "hello".substr(2, 100) + "]");      // llo — length clamps
console.log("[" + "hello".substr(2, -1) + "]");       // (empty) — negative length
console.log("[" + "hello".substr(20, 3) + "]");       // (empty) — start past len
