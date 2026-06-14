// W-O-2 — Object.values(str) returns per-char Str array
// Spec ES §22.1.5.2 + §20.1.2.20: ToObject on a primitive string
// materializes the indexed-property view, whose values are the per-
// char fresh Strs. tora loops __torajs_str_at to mint one fresh Str
// per code unit (same materialize path as W-M-rest; avoids the
// Substr round-trip trap separately solved by FLAG_SUBSTR_VIEW).
// 3 shapes: multi-char "hello" / empty "" / single "x". Bun parity
// verified byte-equal.

console.log(Object.values("hello"));
console.log(Object.values(""));
console.log(Object.values("x"));
