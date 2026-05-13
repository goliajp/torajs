// V3-18 wedge — unary `+s` and `-s` apply ToNumber to a string
// operand per JS spec §13.5.4 / §13.5.5. The `+s` idiom is the
// canonical TS shorthand for `Number(s)`; `-s` is rarer but
// trivially symmetric. Pre-fix tora's typechecker hard-rejected
// `+"42"` / `-"42"` with `\`+\`/\`-\` requires number or
// coercible operand, got String`.
//
// Implementation:
// * check.rs adds Type::String to both UnaryOp::Plus and Neg's
//   accepted operand list, returning Type::Number.
// * ssa_lower's coerce-block (Plus / Neg) detects Str/Substr
//   operands and routes them through __torajs_str_to_number
//   (strtod-based, NaN on parse failure, returns f64). For Neg
//   the downstream F64 branch then emits the sign-preserving
//   FSub from -0.0 — same path that produces -0 from -+0.
// * No new runtime helper needed; we reuse the same
//   __torajs_str_to_number that Number(s) already calls.
//
// BitNot on string is left out: rare in TS code and the
// existing path is already correct for Number / Bool / Null
// — adding it costs the same kind of edit but pulls in i32
// ToInt32 truncation semantics, which is a separate substrate
// item.

// `+s` — the dominant idiom.
console.log(+"42")                   // 42
console.log(+"3.14")                 // 3.14
console.log(+"  5  ")                // 5     ws-trimmed per spec
console.log(+"abc")                  // NaN
console.log(+"")                     // 0     empty → 0 per spec
console.log(+"-0")                   // 0     +"-0" still 0 (unary +)
console.log(+"Infinity")             // Infinity
console.log(+"-Infinity")            // -Infinity

// `-s` — sign-flipping path, exercises the F64 FSub branch.
console.log(-"3")                    // -3
console.log(-"3.14")                 // -3.14
console.log(-"abc")                  // NaN
console.log(-"")                     // -0    (-0 round-trip)
console.log(-"Infinity")             // -Infinity

// Inside expressions — verify the result is genuinely Number,
// not just printable as one.
let s = "7"
console.log(+s + 3)                  // 10
console.log(-s * 2)                  // -14
console.log(+s === 7)                // true (i64↔f64 coerce ok)

// Hex prefix — strtod handles it natively.
// (Binary 0b.. / octal 0o.. prefixes are a separate substrate
// item; Number("0b10") today returns NaN even from bun-aware
// callers because tr's str_to_number leaves those to the
// follow-up wedge that brings tr's helper to spec.)
console.log(+"0xff")                 // 255

// NaN equality — sanity-check the NaN path produces a real NaN.
let nan = +"abc"
console.log(nan !== nan)             // true   NaN-only invariant
console.log(Number.isNaN(nan))       // true
