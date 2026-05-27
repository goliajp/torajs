// Step 8c — ShortStr fast-path for `__torajs_anyv_add` string-
// concat. When the operands are Any-typed strings and the concat
// result fits in ≤ 5 bytes, arith.rs::any_add now skips the
// __torajs_str_concat heap alloc and emits the result as an
// inline ShortStr AnyValue (top16 = 0x0001 + 8-bit len + 5-byte
// payload). When the result exceeds 5 bytes, the existing
// heap-allocating fallback path runs unchanged. Both paths must
// remain byte-equal with bun.
let xs: any[] = ['ab', 'cd', 'abc', 'def', '', 'x']

// Fast-path (≤ 5 bytes): 'ab' + 'cd' = 'abcd', 'a' + 'b' = 'ab',
// 'abc' + 'de' (build mixed via Any[]).
console.log(xs[0] + xs[1])
console.log(xs[5] + xs[5])
console.log(xs[2] + xs[1])

// Empty + nonempty (edge case — empty Str must still round-trip
// correctly through the fast-path).
console.log(xs[4] + xs[0])
console.log(xs[0] + xs[4])
console.log(xs[4] + xs[4])

// Slow-path (> 5 bytes): 'abc' + 'def' = 'abcdef' falls through
// to __torajs_str_concat heap alloc. Result is a Heap+Str cell.
console.log(xs[2] + xs[3])

// Mixed-type concat (numeric Any + string Any) — ToString of the
// number happens inside any_to_str, then concat. The "42" Str
// is len=2 → fast-path eligible.
let n: any = 42
console.log(n + xs[0])
console.log(xs[0] + n)

// typeof on the fast-path result must still be 'string' — the
// 8b shim is_short_str arm must dispatch correctly.
let r: any = xs[0] + xs[1]
console.log(typeof r)
// NOTE: `r.length` is intentionally NOT exercised here — Type::Any
// member access routes through dynobj substrate (see ssa_lower.rs
// `if matches!(obj_ty, Type::Any)` Member arm), which returns
// `undefined` for any string-shaped property (including Heap+Str).
// String-aware Any property dispatch is queued as a separate
// follow-on — pre-existing limitation, not Step 8c scope.
