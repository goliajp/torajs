// String ordering is by UTF-16 code unit (ES §7.2.13 IsLessThan step
// 3.d) — for `<` / `>`, for the default `sort()`, for the ordinal
// `localeCompare` stub, and in the `any` lane.
//
// The compare kernels read the payload BYTES: right for two Latin-1
// strings, wrong as soon as one side is UTF-16. `"世" < "a"` answered
// true (the low byte 0x16 of U+4E16 against 0x61), and
// `["世","a","ab","界"].sort()` put the CJK strings first. Pre-existing;
// found while fixing the split-product sort (rotation 468).

console.log("世" < "a", "a" < "世", "ab" < "世", "世" < "界", "界" < "世");
console.log(["世", "a", "ab", "界"].sort().join("|"));
// Latin-1 above ASCII: é (U+00E9) sorts after z (U+007A)
console.log("é" < "z", "z" < "é", ["é", "z", "a"].sort().join(""));
// a non-BMP character is two code units (D800 DC00): below U+FFFF
console.log("\u{10000}" < "￿", ["￿", "\u{10000}", "퟿"].sort().map(s => s.length).join(","));
// common prefix falls to the length tie-break, across encodings
console.log("ab" < "ab世", "ab世" < "ab", "世" < "世a");
// ordinal localeCompare agrees in sign
console.log("ab".localeCompare("世"), "世".localeCompare("ab"), "世".localeCompare("世"));
// the any lane
let x: any = "世";
let y: any = "a";
console.log(x < y, y < x, x > y);
