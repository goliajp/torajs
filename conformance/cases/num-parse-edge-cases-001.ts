// P12.2 — parseFloat / parseInt edge cases that go beyond the
// basic num-parse-001 / parseint-001-auto-radix coverage.
//
// Audit (2026-06-04 HEAD `70f18f4`): tora already byte-equals bun
// across all 28 advanced cases below. This fixture exists to lock
// in that parity so future substrate refactors can't silently
// break a specific edge (radix=0 sentinel, radix bounds [2,36],
// JS-spec exponent shape, denorm rounding, longest-prefix match).

// ---- parseInt radix sentinel + bounds ----
console.log(parseInt("0x10", 0));    // 16  — radix=0 + 0x prefix → auto-hex
console.log(parseInt("0x10", 10));   // 0   — radix=10 stops at "x"
console.log(parseInt("10", 0));      // 10  — radix=0 no prefix → default 10
console.log(parseInt("0xZZ"));       // NaN — invalid hex chars
console.log(parseInt("99", 16));     // 153 — hex digits, no 0x prefix
console.log(parseInt("FF", 16));     // 255
console.log(parseInt("zZ", 36));     // 1295 — case-insensitive base 36
console.log(parseInt("0", 1));       // NaN — radix < 2
console.log(parseInt("0", 37));      // NaN — radix > 36
console.log(parseInt("123", 2.5));   // 1   — radix truncated to 2

// ---- parseFloat exponent + boundary edges ----
console.log(parseFloat("3.14e+2"));  // 314         — explicit +exp
console.log(parseFloat("3.14e-2"));  // 0.0314
console.log(parseFloat("1e21"));     // 1e+21       — JS spec exponent flip
console.log(parseFloat("1e-7"));     // 1e-7        — JS spec lower boundary
console.log(parseFloat("1.5e"));     // 1.5         — incomplete exp dropped
console.log(parseFloat("Infinityx"));// Infinity    — longest-prefix match

// ---- parseFloat rejects hex / non-decimal prefixes ----
console.log(parseFloat("0x10"));     // 0           — "0" + non-digit "x"

// ---- parseFloat whitespace variants ----
console.log(parseFloat("\n3.14\t")); // 3.14        — \n + \t whitespace strip

// ---- parseFloat overflow / underflow ----
console.log(parseFloat("3e308"));    // 3e+308
console.log(parseFloat("1e309"));    // Infinity    — overflow
console.log(parseFloat("1e-324"));   // 5e-324      — subnormal min
console.log(parseFloat("1e-325"));   // 0           — below denorm
