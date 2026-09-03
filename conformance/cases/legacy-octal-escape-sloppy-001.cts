// annexB §B.1.2 LegacyOctalEscapeSequence — sloppy script goal, where
// these are ordinary spellings rather than the strict-goal SyntaxError.
// tr read `"\101"` as the three characters `101` before rotation 574;
// the production says `A`.
console.log("\101", "\102\103");
// ZeroToThree admits a third digit, FourToSeven does not: `\400` is
// `\40` (a space) followed by the character `0`.
console.log("\377".charCodeAt(0), "\400".length, "\400".charCodeAt(0), "\400".charCodeAt(1));
// NonOctalDecimalEscapeSequence — `\8` / `\9` are the digits themselves.
console.log("\8", "\9");
// A bare `\0` with no digit after it is the §12.9.4.1 NUL escape and
// not legacy at all.
console.log("\0".length, "\0".charCodeAt(0));
// An escaped backslash does not open an escape: `\\101` is a backslash
// then `101`, and `\x5C` likewise contributes no opener.
console.log("\\101", "\x5C101");
