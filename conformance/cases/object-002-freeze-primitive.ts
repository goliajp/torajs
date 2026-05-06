// T-09.d v0.4.0 fix — Object.freeze / Object.isFrozen on primitives
// MUST short-circuit at compile time (no runtime helper call). The
// helpers deref `p` as a heap header — a primitive bit pattern (e.g.
// `true` is the i64 1) makes that SIGSEGV. Per ES2015 spec these
// calls are no-ops on primitives: freeze returns the value unchanged,
// isFrozen returns true. test262 15.2.3.9-1-3 / 15.2.3.9-1-4 /
// 15.2.3.12-1-3 cover this.
//
// Also covers: Object.freeze on a static-literal string (writing the
// FROZEN bit to .rodata SIGBUSs) — the C-side helper now skips the
// bit set when STATIC_LITERAL is present.

console.log(Object.isFrozen(true))
console.log(Object.isFrozen(false))
console.log(Object.freeze(true))
console.log(Object.freeze(42))
console.log(Object.freeze(3.14))

// String literal — was SIGBUS on .rodata flag write.
Object.freeze('abc')
console.log('ok-1')
console.log(Object.isFrozen('abc'))

// Function expression — heap closure, regular freeze path.
let f = function (): number {
  return 1
}
Object.freeze(f)
console.log(Object.isFrozen(f))
console.log('done')
