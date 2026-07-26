// ES2024 §12.7.1 — identifiers are Unicode, not ASCII. IdentifierStartChar
// is UnicodeIDStart (plus `$` / `_`); IdentifierPartChar is UnicodeIDContinue
// plus the two joiners the spec names by hand, ZWNJ and ZWJ.

// Plain letters outside ASCII, in a few scripts.
let café = 1;
let 中文 = 2;
let αβγ = 3;
console.log(café + 中文 + αβγ);

// Other_ID_Start: neither is in the Letter category, and both are
// identifier-startable anyway. test262's class-elements privatename
// files lean on exactly these two.
let ℘ = 4;
let ℮ = 5;
console.log(℘ * ℮);

// ID_Continue that is not ID_Start: a combining mark (U+0301) and a
// non-ASCII digit (U+0660, Nd) can continue a name, not begin one.
let á = 6;
let b٠ = 7;
console.log(á + b٠);

// ZWNJ / ZWJ mid-identifier.
let ZW_‌_NJ = 8;
let ZW_‍_J = 9;
console.log(ZW_‌_NJ + ZW_‍_J);

// The same rules apply to private names, object keys, and method names.
class C {
  #℘ = 10;
  中文 = 11;
  αmethod(): number {
    return this.#℘ + this.中文;
  }
}
console.log(new C().αmethod());

const obj = { café: 12, 中文: 13, µ(): number { return 14; } };
console.log(obj.café + obj.中文 + obj.µ());

console.log(typeof 中文, typeof ℘);
