// String.prototype.trim family -- ES 22.1.3.32.1 TrimString Unicode
// whitespace set (WhiteSpace + LineTerminator), Latin-1 + UTF-16
// payloads, plus the StringToNumber / parseInt / parseFloat
// consumers of the same set.
// RFC 20260712-string-proto-cluster chunk A.

// Latin-1 payload: NBSP (U+00A0) on both sides.
console.log(JSON.stringify("\u00A0abc\u00A0".trim()));
console.log(JSON.stringify("\u00A0abc\u00A0".trimStart()));
console.log(JSON.stringify("\u00A0abc\u00A0".trimEnd()));

// UTF-16 payload: separators + ideographic space + ZWNBSP.
console.log(JSON.stringify("\u2028abc\u2029".trim()));
console.log(JSON.stringify("\u3000a b\u3000".trim()));
console.log(JSON.stringify("\uFEFFabc\uFEFF".trim()));
console.log(JSON.stringify("\u000A\u000D\u2028\u2029".trim()));

// The full test262 15.5.4.20-3-2 whitespace string trims to empty.
console.log(JSON.stringify("\u0009\u000A\u000B\u000C\u000D\u0020\u00A0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF".trim()));

// Negative: ZWSP (U+200B) and MVS (U+180E, post-Unicode-6.3) are
// NOT whitespace -- must survive.
console.log(JSON.stringify("\u200Bab\u200B".trim()));
console.log(JSON.stringify("\u180Eab".trim()));

// Substr view trim (slice first, then trim the view).
const s = "xx\u00A0hi\u3000yy";
console.log(JSON.stringify(s.slice(2, 7).trim()));

// StringToNumber shares the TrimString set.
console.log(Number("\u00A01\u00A0"));
console.log(Number("\u30002\u3000"));
console.log(Number("\uFEFF3"));
console.log(Number("\u00A0"));
console.log(+"\u20284\u2029");
console.log(Number("\u200B5"));

// parseInt / parseFloat trim leading TrimString whitespace.
console.log(parseInt("\u00A042"));
console.log(parseInt("\u300042xyz"));
console.log(parseFloat("\u00A03.5"));
console.log(parseFloat("\u30003.5\u4E00"));

// Canonical-encoding invariant: a UTF-16 source trimmed down to
// all-Latin-1 content must compare equal to the Latin-1 literal.
console.log("\u2028abc".trim() === "abc");
console.log("\u3000abc".trimStart() === "abc");
console.log("abc\uFEFF".trimEnd() === "abc");
