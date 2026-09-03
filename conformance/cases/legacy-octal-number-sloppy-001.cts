// annexB §B.1.1 LegacyOctalIntegerLiteral / NonOctalDecimalInteger-
// Literal — sloppy script goal. tr read `010` as ten before rotation
// 574; the production says eight.
console.log(010, 0777, 017);
// One `8` or `9` anywhere in the run turns the whole literal decimal.
console.log(08, 09, 0778, 0888);
// The leading zero has to be followed by a digit to be legacy at all —
// these keep every meaning they had.
console.log(0, 0.5, 0e1, 0x10, 0o17, 0b11, 1_000, 0.010);
