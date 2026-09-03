// §12.9.6 refuses a LegacyOctalEscapeSequence in an UNTAGGED template
// (rotation 574). The shapes that must NOT be caught by that gate,
// because each of them is legal:
// a tagged template's raw text keeps the spelling verbatim,
function raw0(s: any) { return s.raw[0] }
console.log(raw0`\101`, String.raw`\101`);
// an escaped backslash opens no escape at all,
console.log(`\\101`, `\\8`);
// and `\0` followed by a non-digit is the §12.9.4.1 NUL escape.
console.log(`x\0y`.length, `\0`.charCodeAt(0));
// Ordinary templates keep every meaning they had.
const n = 5;
console.log(`v=${n}`, `a\tb`, `\x41`, `B`, `${1}${2}`);
