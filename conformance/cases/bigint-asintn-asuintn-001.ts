// P12.4-B/C — BigInt.asIntN(bits, value) / asUintN(bits, value) per
// ES §21.2.2.1 / §21.2.2.2. Substrate ship in `3b73546`:
// - New `__torajs_bigint_as_int_n` / `__torajs_bigint_as_uint_n` runtime
// - SSA static dispatch for `BigInt.<m>(bits, value)`
// - check.rs typecheck arm for the static signature
//
// Coverage: bits in [0, 64]. bits > 64 deliberately throws RangeError
// (tracked as L3b — no real-world case needs it; tora's Number is
// integer-typed and the spec bound `bits <= 2^53-1` is wider than ever
// useful for fixed-width bitfield truncation).

// ---- asIntN: in-range positive ----
console.log(BigInt.asIntN(8, 0n));               // 0n
console.log(BigInt.asIntN(8, 1n));               // 1n
console.log(BigInt.asIntN(8, 127n));             // 127n  (max +)

// ---- asIntN: top bit flips sign ----
console.log(BigInt.asIntN(8, 128n));             // -128n  (min -)
console.log(BigInt.asIntN(8, 255n));             // -1n
console.log(BigInt.asIntN(8, 256n));             // 0n     (mod 2^8)

// ---- asIntN: negative input ----
console.log(BigInt.asIntN(8, -1n));              // -1n
console.log(BigInt.asIntN(8, -127n));            // -127n
console.log(BigInt.asIntN(8, -128n));            // -128n
console.log(BigInt.asIntN(8, -129n));            // 127n

// ---- asIntN: 16/32/64 bit widths ----
console.log(BigInt.asIntN(16, 32767n));          // 32767n
console.log(BigInt.asIntN(16, 32768n));          // -32768n
console.log(BigInt.asIntN(32, 2147483647n));     // 2147483647n
console.log(BigInt.asIntN(32, 2147483648n));     // -2147483648n
console.log(BigInt.asIntN(64, 9223372036854775807n));        // 9223372036854775807n
console.log(BigInt.asIntN(64, 9223372036854775808n));        // -9223372036854775808n
console.log(BigInt.asIntN(64, -9223372036854775808n));       // -9223372036854775808n

// ---- asIntN: 2^63 boundary ----
console.log(BigInt.asIntN(64, 2n ** 63n));       // -9223372036854775808n

// ---- asUintN: in-range positive ----
console.log(BigInt.asUintN(8, 0n));              // 0n
console.log(BigInt.asUintN(8, 127n));            // 127n
console.log(BigInt.asUintN(8, 255n));            // 255n  (max u8)

// ---- asUintN: overflow wraps ----
console.log(BigInt.asUintN(8, 256n));            // 0n
console.log(BigInt.asUintN(8, 257n));            // 1n
console.log(BigInt.asUintN(8, 65535n));          // 255n

// ---- asUintN: negative input wraps to unsigned ----
console.log(BigInt.asUintN(8, -1n));             // 255n
console.log(BigInt.asUintN(8, -128n));           // 128n
console.log(BigInt.asUintN(16, -1n));            // 65535n
console.log(BigInt.asUintN(32, -1n));            // 4294967295n
console.log(BigInt.asUintN(64, -1n));            // 18446744073709551615n
console.log(BigInt.asUintN(64, 2n ** 64n));      // 0n

// ---- bits == 0 ----
console.log(BigInt.asIntN(0, 100n));             // 0n
console.log(BigInt.asUintN(0, 100n));            // 0n
console.log(BigInt.asIntN(0, -1n));              // 0n
console.log(BigInt.asUintN(0, -1n));             // 0n

// bits > 64 is L3b backlog: tora throws RangeError; bun handles arbitrary
// bits via word-level masking. Not covered by this fixture to keep
// bun-byte-equal — see plan-state P12.4 L3b note.
