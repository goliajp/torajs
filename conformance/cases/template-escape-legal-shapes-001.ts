// §12.9.6 TemplateCharacter (rotation 577). The refusals — a malformed
// `\x` / `\u` in an UNTAGGED template — are negative cases and live in
// test262. This fixture is the other side: the shapes next to that
// gate that are LEGAL and must keep working.
//
// Every well-formed spelling still cooks, in both segments and across
// an interpolation.
const w = "w";
console.log(`\x41`, `A`, `\u{1F639}`.length, `aB${w}\x43d`);
// A LineContinuation and the ordinary character escapes.
console.log(`a\
b`, `x\ty`.length, `\``, `\${notasub}`);
// A NonEscapeCharacter is still itself; `q` is not an EscapeCharacter.
console.log(`\q`, `\0`.charCodeAt(0));
// A TAGGED template is allowed the spellings an untagged one refuses
// (§12.9.6 gives it a cooked value of `undefined` instead of an
// error), so the gate must not close on it. The raw text is the TRV
// and keeps the spelling verbatim.
const raws = (s: any): string => s.raw.join("|");
console.log(raws`\u0`, raws`\x0`, raws`\u{g`, raws`\u{10FFFFF}`);
console.log(raws`\u0${w}tail`);
// A tag also sees well-formed escapes raw, while cooked decodes them.
const both = (s: any): string => `${s.raw[0]}/${s[0]}`;
console.log(both`A`, both`a\tb`);
