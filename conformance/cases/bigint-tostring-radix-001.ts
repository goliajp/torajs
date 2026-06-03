// P12.4 — BigInt.prototype.toString(radix) per ES §6.1.6.2.13.
//
// Pre-fix tora rejected the optional radix arg at the typechecker:
// `type error: expected 0 argument(s), got 1` for any
// `(n).toString(2)` form. Substrate fix in `78efe91`:
// - tostring.rs refactored to take radix via private to_string_radix
//   helper + new __torajs_bigint_to_string_radix extern
// - check.rs split toString sig to (radix?: Any) → String
// - ssa_lower.rs dispatch branches on arg count and routes to the
//   new intrinsic with f64-to-i64 coercion when needed

// Default decimal (no arg) — must keep working
console.log((123n).toString());        // "123"
console.log((-123n).toString());       // "-123"
console.log((0n).toString());          // "0"

// All supported bases
console.log((123n).toString(2));       // "1111011"
console.log((123n).toString(8));       // "173"
console.log((123n).toString(10));      // "123"
console.log((123n).toString(16));      // "7b"
console.log((123n).toString(36));      // "3f"

// Sign preservation in non-decimal
console.log((-123n).toString(2));      // "-1111011"
console.log((-123n).toString(16));     // "-7b"
console.log((-1024n).toString(2));     // "-10000000000"

// Edge: zero in non-decimal
console.log((0n).toString(2));         // "0"
console.log((0n).toString(16));        // "0"
console.log((0n).toString(36));        // "0"

// Edge: smallest non-zero
console.log((1n).toString(2));         // "1"
console.log((1n).toString(36));        // "1"

// Power-of-2 boundaries (chunk boundary stress test)
console.log((255n).toString(16));      // "ff"
console.log((256n).toString(16));      // "100"
console.log((1024n).toString(2));      // "10000000000"
console.log((2n ** 32n).toString(16)); // "100000000"
console.log((2n ** 64n).toString(16)); // "10000000000000000"

// Multi-chunk magnitudes (>= 2 u64 limbs)
console.log((2n ** 100n).toString(2)); // "1" followed by 100 zeros
console.log((10n ** 30n).toString(36));// large base-36

// All bases for a non-trivial value
console.log((12345678n).toString(2));
console.log((12345678n).toString(7));
console.log((12345678n).toString(16));
console.log((12345678n).toString(36));

// Spec lowercase alphabet (case sensitivity)
console.log((10n).toString(16));       // "a" lowercase
console.log((35n).toString(36));       // "z" lowercase
