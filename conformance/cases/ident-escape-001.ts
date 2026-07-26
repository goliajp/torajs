// ES2024 §12.7.1 — an identifier may spell any of its characters as a
// `\u` escape, and §12.7.2 says the escape may not manufacture a keyword.
// A name is its code points: the escaped and literal spellings of the
// same character name the same binding.

let \u{6F} = 1;
o += 1;
console.log(o);

// Both UnicodeEscapeSequence forms, on characters that are plain ASCII.
let \u{62}a\u0072 = 2;
console.log(bar);

// Escapes reach the non-ASCII half too, including Other_ID_Start.
let \u2118 = 3;
console.log(℘);

// A supplementary code point, spelled as a pair and as a braced escape.
let \u{1D400} = 4;
console.log(𝐀);
let \ud835\udc002 = 5;
console.log(𝐀2);

// Mid-identifier, on a joiner that only ID_Continue admits.
let ZW\u200C_NJ = 6;
console.log(ZW‌_NJ);

// Private names, class fields, object keys and method names all take the
// same production.
class C {
  #\u{6F} = 7;
  \u{62}ar = 8;
  \u{6D}ethod(): number {
    return this.#\u{6F} + this.bar;
  }
}
console.log(new C().method());

const obj = { \u{6B}ey: 9, \u2118: 10 };
console.log(obj.key + obj.℘);

// `type` is contextual, not a ReservedWord — escaping it yields a plain
// identifier rather than the syntax error a real keyword would give.
// (Spelled escaped on both sides: tr still lexes a bare `type` as the TS
// keyword everywhere, which is a separate gap.)
let \u{74}ype = 11;
console.log(\u{74}ype);
