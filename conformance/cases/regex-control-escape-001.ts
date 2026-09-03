// §22.2.1 CharacterEscape :: `c` ControlLetter + ClassEscape's
// IdentityEscape (rotation 577). ControlLetter is [A-Za-z] and the
// value is the letter's code modulo 32; annexB §B.1.4 widens the set
// to DecimalDigit | `_` INSIDE A CLASS and outside u/v, and nowhere
// else.
const ch = (n: number): string => String.fromCharCode(n);
// The escape itself, in an atom and in a class, with and without `u`.
console.log(/\cA/.test(ch(1)), /[\cA]/.test(ch(1)));
console.log(/\cA/u.test(ch(1)), /[\cA]/u.test(ch(1)));
console.log(/\cz/.test(ch(26)), /[\cM]/.test(ch(13)));
// ...and it is NOT the letters themselves.
console.log(/\cA/.test("cA"), /[\cA]/.test("c"));
// annexB widens the class set outside u/v only.
console.log(/[\c1]/.test(ch(0x11)), /[\c_]/.test(ch(0x1F)));
// Outside a class the widening does not apply, so annexB reads the
// BACKSLASH as the atom and lets `c` reparse: `\c1` is three literal
// characters.
console.log(/^(?:\c1)$/.test("\\c1"), /^(?:\c%)$/.test("\\c%"));
// The same reading inside a class puts the backslash in the set.
console.log(/[\c%]/.test("\\"), /[\c%]/.test("c"), /[\c%]/.test("%"));
// A ClassEscape under u/v admits SyntaxCharacter, `/`, and the `-`
// that ClassEscape itself adds; `\b` is still backspace there.
console.log(/[\-]/u.test("-"), /[\/]/u.test("/"), /[\]]/u.test("]"));
console.log(/[\b]/u.test(ch(8)));
// Outside u/v the lenient reading stands.
console.log(/[\q]/.test("q"), /[\1]/.test(ch(1)));
