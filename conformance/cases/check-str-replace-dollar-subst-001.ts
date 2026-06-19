// ES §22.1.3.18.5 GetSubstitution — string-needle case. Tora's
// runtime `__torajs_str_replace` / `_replace_all` previously
// treated the replacement template as a literal byte sequence, so
// `"aaa".replace("a", "$&$&")` produced "$&$&aa" instead of bun-
// parity "aaaa". Now the runtime detects `$` via a single byte scan
// (template_has_dollar) and routes to a per-match substitution
// expander; dollar-free templates skip the path entirely so the
// existing zero-allocation fast path is preserved.
//
// Patterns:
//   $$    → "$"
//   $&    → matched substring
//   $`    → portion of s before the match
//   $'    → portion of s after the match
//   $N    → preserved literally (string needle: captures empty, so
//           `$N` is out of range per spec). bun mirrors this.
//   $<n>  → preserved literally (string needle: namedCaptures
//           undefined).

// $$ — emit literal `$`
console.log("a".replace("a", "$$"));
console.log("ab".replace("a", "$$x"));

// $& — matched substring
console.log("aaa".replace("a", "$&"));
console.log("aaa".replace("a", "$&$&"));
console.log("abc".replace("b", "($&)"));
console.log("aaa".replaceAll("a", "$&$&"));

// $` — pre-match portion
console.log("axb".replace("x", "$`"));
console.log("123xyz456".replace("xyz", "[$`]"));

// $' — post-match portion
console.log("axb".replace("x", "$'"));
console.log("123xyz456".replace("xyz", "[$']"));

// Mixed in one template
console.log("axb".replace("x", "<$`|$&|$'>"));

// $N — collapse to "" (string needle has no captures)
console.log("ab".replace("a", "$1"));
console.log("ab".replace("a", "$12"));
console.log("ab".replace("a", "X$1Y"));

// $<name> — collapse to "" (no named captures)
console.log("ab".replace("a", "$<foo>"));
console.log("ab".replace("a", "X$<foo>Y"));

// Unrecognized — preserve literally (per spec)
console.log("ab".replace("a", "$Z"));

// Trailing $ — preserve literally
console.log("ab".replace("a", "end$"));

// replaceAll with $`/$' is per-match
console.log("axbxc".replaceAll("x", "($`)"));
console.log("axbxc".replaceAll("x", "($')"));

// Dollar-free templates — fast path, regression check.
console.log("abc".replace("b", "X"));
console.log("aaa".replaceAll("a", "b"));
console.log("hello".replace("ll", "LL"));
