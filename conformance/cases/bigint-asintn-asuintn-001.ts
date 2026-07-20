// P12.4-B/C — BigInt.asIntN(bits, value) / asUintN(bits, value) per
// ES §21.2.2.1 / §21.2.2.2. Substrate ship in `3b73546`:
// - New `__torajs_bigint_as_int_n` / `__torajs_bigint_as_uint_n` runtime
// - SSA static dispatch for `BigInt.<m>(bits, value)`
// - check.rs typecheck arm for the static signature
//
// Coverage: arbitrary bits >= 0 (multi-limb masking shipped with RFC
// 20260720-ctor-static-reflection 刀 5a; the old [0, 64] cap is gone).

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

// ---- bits > 64: multi-limb masking ----
console.log(BigInt.asUintN(65, -1n));            // 36893488147419103231n (2^65 - 1)
console.log(BigInt.asUintN(128, -1n));           // 340282366920938463463374607431768211455n
console.log(BigInt.asUintN(100, 2n ** 100n + 5n)); // 5n
console.log(BigInt.asIntN(65, 2n ** 64n));       // -18446744073709551616n
console.log(BigInt.asIntN(128, 2n ** 127n));     // -170141183460469231731687303715884105728n
console.log(BigInt.asIntN(128, 2n ** 127n - 1n)); // 170141183460469231731687303715884105727n

// ---- huge bits: identity fast path (no huge allocation) ----
console.log(BigInt.asUintN(1000000, 123n));      // 123n
console.log(BigInt.asIntN(1000000, -123n));      // -123n
