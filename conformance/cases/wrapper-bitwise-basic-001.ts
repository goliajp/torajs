// RFC 20260716-primitive-wrapper-substrate 刀 7 — bitwise on
// primitive wrappers. Closes the sweep-flagged pass→incompat
// regression cluster (`bitwise-and/-not 遇 new Boolean(x) & prim`
// 6+ test262 cases) rotation 112 turned up: the checker rejected
// `Any & Prim` because check_bitwise / check_ushr / check_bit_not
// had no Any arm — mirror of the arith / compare Any-fringe.
//
// Runtime path: `__torajs_anyv_bitwise_pair` (BitAnd/BitOr/BitXor/
// Shl/Shr/UShr) + `__torajs_anyv_bitnot_pair` (~). Both operands
// route ToNumber (which unwraps [[NumberData]] / [[BooleanData]]
// / [[StringData]] via the primitive-wrapper substrate 刀 1-6),
// then ToInt32 (ToUint32 on `>>>`'s LHS), then 32-bit op.

// BooleanWrapper — ToNumber(true) = 1, ToNumber(false) = 0.
console.log(new Boolean(true) & 1);          // 1
console.log(new Boolean(false) & 1);         // 0
console.log(new Boolean(true) | 0);          // 1
console.log(new Boolean(false) ^ 1);         // 1
console.log(~new Boolean(true));             // -2 (~1)
console.log(~new Boolean(false));            // -1 (~0)

// Strict-eq with primitive Number — dispatches through any_strict_eq
// so the Any-boxed I32 result compares equal to a primitive Number.
console.log((new Boolean(true) & 1) === 1);  // true
console.log((new Boolean(false) & 1) === 0); // true

// NumberWrapper — ToNumber(new Number(x)) = x, then ToInt32.
console.log(new Number(5) & 3);              // 1 (5 & 3)
console.log(new Number(5) | 2);              // 7
console.log(new Number(-1) & 0xFF);          // 255 (ToInt32(-1) & 0xFF = 0xFFFFFFFF & 0xFF)
console.log(~new Number(0));                 // -1

// Shifts — new Number(4) << 1 = 8; UShr result ∈ [0, 2^32).
console.log(new Number(4) << 1);             // 8
console.log(new Number(16) >> 2);            // 4
console.log(new Number(-1) >>> 0);           // 4294967295 (UShr LHS ToUint32)

// StringWrapper — ToNumber(new String("3")) = 3, then ToInt32.
console.log(new String("5") & 3);            // 1
console.log(~new String("0"));               // -1
console.log(new String("abc") | 0);          // 0 (NaN → ToInt32 = 0)

// Wrapper both sides.
console.log(new Number(6) & new Number(3));  // 2
console.log(new Boolean(true) | new Boolean(false)); // 1
