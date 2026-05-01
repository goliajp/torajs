// Adapted from test262: built-ins/Math/{imul,clz32,fround}/* — three
// 32-bit-flavored Math statics. imul / clz32 take i64 args (no f64
// coercion); fround takes f64. Implementations live in runtime_str.c
// and exploit the host's libc / __builtin_clz.
function check(): number {
  // Math.imul — 32-bit signed multiply with low-32 truncation.
  if (Math.imul(2, 3) !== 6) { throw "#1"; }
  if (Math.imul(-2, 3) !== -6) { throw "#2"; }
  if (Math.imul(0, 999) !== 0) { throw "#3"; }
  // Truncation: 0xffffffff * 2 == 0xfffffffe in low 32 bits, sign-
  // extended (which is -2 as a signed i32).
  if (Math.imul(0xffffffff, 2) !== -2) { throw "#4: low-32 wrap"; }

  // Math.clz32 — leading zeros of x's i32 representation.
  if (Math.clz32(0) !== 32) { throw "#5: zero"; }
  if (Math.clz32(1) !== 31) { throw "#6"; }
  if (Math.clz32(2) !== 30) { throw "#7"; }
  if (Math.clz32(0xff) !== 24) { throw "#8"; }
  if (Math.clz32(0xffff) !== 16) { throw "#9"; }
  if (Math.clz32(0x80000000) !== 0) { throw "#10: high bit set"; }

  // Math.fround — round to nearest f32 then back to f64. Values that
  // are exactly representable in f32 round to themselves.
  if (Math.fround(0) !== 0) { throw "#11"; }
  if (Math.fround(0.5) !== 0.5) { throw "#12"; }
  if (Math.fround(0.25) !== 0.25) { throw "#13"; }
  if (Math.fround(1) !== 1) { throw "#14"; }
  if (Math.fround(-2) !== -2) { throw "#15"; }
  return 0;
}
console.log(check());
