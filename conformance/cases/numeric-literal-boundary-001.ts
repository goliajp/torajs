// ES §12.9.3 — "The SourceCharacter immediately following a
// NumericLiteral must not be an IdentifierStart or DecimalDigit."
//
// The rejections it adds have no stdout to compare, so this fixture
// guards the other half: every spelling that stays legal, and in
// particular the ones that LOOK like they should trip the rule.
console.log(1..toString());        // `1.` then `.toString` — the next
console.log((1.).toFixed(2));      // character is `.`, not an ident
console.log(.5 + 1.);              // leading and trailing dot forms

console.log(0x1f, 0b101, 0o17);    // radix literals end at a non-digit
console.log(1_000_000);            // separators are inside the literal
console.log(1.5e3, 1e-2, 1E+2);    // exponents carry their own letter
console.log(10n + 5n);             // the BigInt `n` is part of it

console.log(3 in [1, 2, 3, 4]);    // `3 in` needs the space; `3in` does not lex
for (var i = 0; i < 2; i++) {}
console.log(i);
