// P11.1-S5 — Substr.length spec parity.
//
// Pre-S5: tora's Substr stored `len@8` as a byte count, so
// `.charAt(1).length` returned the byte size of the parent code
// unit (2 for "文" / "😀" half) instead of bun's `1`. S5 flips
// `len@8` and `offset@24` to JS code units so `.length` matches
// spec and bun on multi-byte source strings.
//
// Coverage: BMP CJK (1 cu per char), surrogate pair (2 cu per
// codepoint), Latin-1 ASCII sanity.

const s = "中文abc";
console.log(s.charAt(0).length);
console.log(s.charAt(1).length);
console.log(s.charAt(2).length);
console.log(s.length);

const t = "😀ab";
console.log(t.charAt(0).length);
console.log(t.charAt(1).length);
console.log(t.length);

const u = "hello";
console.log(u.charAt(2).length);
console.log(u.length);
