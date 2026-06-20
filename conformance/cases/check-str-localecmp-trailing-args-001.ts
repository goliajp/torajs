// S238 — `String.localeCompare(other, locales?, options?)` per ES
// §22.1.3.10: the spec reserves the trailing `locales` and `options`
// slots for Intl-aware comparison. tora's bytewise helper has no
// locale awareness, so trailing args are ignored — matches bun for
// the byte-comparable receivers. The check.rs S238 carve-out widens
// the 1-arg-only declared signature to accept 1/2/3 args; the
// ssa_lower_str loop breaks after the first arg so the helper's
// (Str, Str) ABI never sees the extra operands.
console.log("a".localeCompare("b"));
console.log("a".localeCompare("b", undefined));
console.log("a".localeCompare("b", undefined, undefined));
console.log("a".localeCompare("b", "en"));
console.log("a".localeCompare("a", "en"));
console.log("b".localeCompare("a", "en", undefined));
