// S144 — Number("0b...") / Number("0o...") per ES §7.1.4.1.1
// (BinaryIntegerLiteral / OctalIntegerLiteral). Pre-fix
// __torajs_str_to_number only recognized the hex `0x` prefix
// (`0b101` / `0o17` came back NaN). Now both binary (`0b...` /
// `0B...`) and octal (`0o...` / `0O...`) parse the same way: ≥1
// valid digit after the 2-char prefix → u64 → f64.

// canonical decimal value of each prefix form
console.log("1:", Number("0b101"));
console.log("2:", Number("0o17"));
console.log("3:", Number("0B11111111"));
console.log("4:", Number("0O777"));
console.log("5:", Number("0x10"));

// surrounding whitespace trims (matches existing decimal / hex
// behavior)
console.log("6:", Number("  0b101  "));
console.log("7:", Number("\t0o17\n"));

// no-digit / out-of-radix / non-utf8-like → NaN (spec compliant)
console.log("8:", Number("0b"));
console.log("9:", Number("0o"));
console.log("10:", Number("0b2"));
console.log("11:", Number("0o9"));

// unary plus uses the same path
console.log("12:", +"0b1010");
console.log("13:", +"0o20");
