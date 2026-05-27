// Step 8d — IR-side const ShortStr u64 emit for compile-time short
// string literals in Any context. When ssa_lower's box_to_any_from_expr
// sees a string literal whose UTF-8 bytes fit in ≤ SHORT_STR_CAP (= 5),
// it skips the runtime `__torajs_anyv_box_from_pair(4, str_ptr)` call
// and emits `IntToPtr(ConstI64(short_u64))` typed as Any directly. The
// short_u64 is computed at compile time via
// `torajs_anyvalue::try_box_short_str(bytes)` (NaN-box ShortStr top16
// = 0x0001 + 8-bit len + 5-byte payload). When the bytes exceed 5, the
// literal falls through to the existing any_box(4, str_ptr) heap path.
//
// Acceptance: bun-byte-equal output. The optimization is observable
// only via LLVM IR dump (no runtime `call __torajs_anyv_box_from_pair`
// for the short literals); the user-visible semantics are identical
// — typeof + content + concat all behave exactly as Heap+Str.

// Bytes-len coverage 0..5 — full SHORT_STR_CAP range.
let xs: any[] = ['', 'a', 'ab', 'abc', 'abcd', 'abcde']
for (let i = 0; i < xs.length; i = i + 1) {
  console.log(typeof xs[i], xs[i])
}

// Boundary: 6 bytes falls through to Heap+Str. typeof still 'string'.
let big: any = 'abcdef'
console.log(typeof big, big)

// Multi-byte UTF-8 ≤ 5 bytes: '中' = 3 bytes, '中文' = 6 bytes
// (fallthrough). Bytes-only ShortStr inherits Str layout, so concat
// + typeof + console.log all round-trip the same bytes.
let zh1: any = '中'
let zh2: any = '中文'
console.log(typeof zh1, zh1)
console.log(typeof zh2, zh2)

// Mixed: short literal + non-short literal in the same Any[] — exercises
// the conditional dispatch (literal vs not) inside box_to_any_from_expr.
let mix: any[] = ['ok', 'longer-string', 'x', '']
for (let i = 0; i < mix.length; i = i + 1) {
  console.log(typeof mix[i], mix[i])
}

// Concat of two ShortStr-emitted literals — exercises 8c-3 fast-path on
// the producer side (both operands now arrive as inline ShortStr Any
// values via 8d, then any_add string-concat tries try_concat_short).
let r: any = mix[0] + mix[2]
console.log(typeof r, r)
