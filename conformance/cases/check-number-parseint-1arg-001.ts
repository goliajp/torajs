// V3-18 wedge — `Number.parseInt(s)` accepts the 1-arg form
// per JS spec §21.1.2.13 (radix is optional; bare 1-arg
// auto-detects 0x prefix → 16, otherwise 10). Pre-fix tora's
// check.rs declared `Number.parseInt` as
// `Function([String, Number], Number)` so the 1-arg call
// failed at the unified arity check with 'expected 2
// argument(s), got 1'. The global `parseInt(s)` form already
// worked (it had its own special-case handler from a previous
// wedge); this commit gives `Number.parseInt` the same
// treatment.
//
// SSA lower already supported the 1-arg shape (passed
// ConstI64(0) as the auto-detect radix sentinel) — only
// check.rs was rejecting at the type level.
//
// Implementation: in check.rs add a sibling Member-call
// special-case before the generic Function-arity check,
// mirroring the global parseInt handler. Accepts 1-2 args
// with the standard String/Number arg types and returns
// Number.

// 1-arg form — auto-detect base.
console.log(Number.parseInt("99"))             // 99
console.log(Number.parseInt("123"))            // 123
console.log(Number.parseInt("0xff"))           // 255
console.log(Number.parseInt("0"))              // 0

// 2-arg form — explicit radix (was already accepted pre-fix).
console.log(Number.parseInt("ff", 16))         // 255
console.log(Number.parseInt("11", 2))          // 3
console.log(Number.parseInt("777", 8))         // 511
console.log(Number.parseInt("99", 10))         // 99

// Negative.
console.log(Number.parseInt("-42"))            // -42
console.log(Number.parseInt("-ff", 16))        // -255

// Wrapper-style usage in a numeric pipeline.
function bumpDigits(s: string): number {
  return Number.parseInt(s) + 1
}
console.log(bumpDigits("9"))                   // 10
console.log(bumpDigits("99"))                  // 100
