// A string literal spelling a lone surrogate keeps that code unit:
// the string value is a sequence of UTF-16 code units (§6.1.4), and
// the compiler must not fold `"\uD800"` to U+FFFD on the way to
// .rodata. Both escape forms, both halves, and the pair-join rule.
const hi = "\uD800";
const lo = "\uDFFF";
const braced = "\u{DC00}";
console.log(hi.length, hi.charCodeAt(0), lo.charCodeAt(0), braced.charCodeAt(0));
console.log(hi === String.fromCharCode(0xd800), lo === String.fromCharCode(0xdfff));
console.log(hi.codePointAt(0), "a\uD800b".length, "a\uD800b".charCodeAt(1));
// an escaped pair is one code point, equal to the raw character
console.log("😀" === "😀", "\u{D83D}\u{DE00}" === "😀", "😀".length);
// a low then a high do not join
console.log("\uDE00\uD83D".length, "\uDE00\uD83D".codePointAt(0), "\uDE00\uD83D".codePointAt(1));
// case mapping leaves a lone surrogate alone (§22.1.3.29 / .30)
console.log(hi.toUpperCase() === hi, lo.toLowerCase() === lo, "a\uD800B".toUpperCase().charCodeAt(1));
// JSON.stringify well-formed output (§25.5.2.2 QuoteJSONString)
console.log(JSON.stringify(hi), JSON.stringify("x\uDFFFy"));
// encodeURI refuses a lone surrogate
try {
  encodeURI(hi);
  console.log("no throw");
} catch (e) {
  console.log((e as Error).name);
}
// concatenation at runtime joins a pair across the seam
const joined = hi + "\uDC00";
console.log(joined.length, joined.codePointAt(0), joined === "\u{10000}");
// property keys spelled with a lone surrogate still round-trip
const o: any = {};
o[hi] = 1;
console.log(Object.keys(o).length, Object.keys(o)[0].charCodeAt(0), o["\uD800"]);
