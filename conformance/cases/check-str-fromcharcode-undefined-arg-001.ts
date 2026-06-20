// S231 — `String.fromCharCode(undefined)` per ES §22.1.2.1:
// ToUint16(undefined) = 0, so the 1-arg undef shape yields a one-char
// NUL string. The check.rs S231 carve-out widens the arg-type gate
// and ssa_lower substitutes ConstI64(0) into
// `__torajs_str_from_char_code` so the ConstPtrNull undef sentinel
// never reaches the helper's i64 ABI.
const s = String.fromCharCode(undefined);
console.log(s.length);
console.log(s.charCodeAt(0));
