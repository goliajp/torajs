// Every power of two a double can hold, round-tripped through
// Math.pow and Math.log2 — including the subnormal ones below
// 2^-1022, which two separate fast paths used to miss.
//
// `powi` computed a negative exponent as `1 / x^|n|`, and `2^1074`
// is infinity, so every exponent past -1023 answered 0. `log2`'s
// exact-power-of-two test asked for zero mantissa bits, which is
// what a NORMAL power of two looks like; a subnormal one carries a
// single mantissa bit instead and fell through to `log(x) / ln(2)`.
let mismatches = 0;
for (let i = -1074; i < 1024; i++) {
  if (Math.log2(Math.pow(2, i)) !== i) {
    mismatches = mismatches + 1;
    if (mismatches < 4) {
      console.log("mismatch at", i, Math.pow(2, i), Math.log2(Math.pow(2, i)));
    }
  }
}
console.log("mismatches", mismatches);

console.log(Math.pow(2, -1074), Number.MIN_VALUE, Math.pow(2, -1074) === Number.MIN_VALUE);
console.log(Math.pow(2, -1075), Math.pow(2, -1073), Math.pow(0.5, 1074));
console.log(Math.log2(Number.MIN_VALUE), Math.log2(Math.pow(2, -1066)));
console.log(Math.pow(2, 1024), Math.pow(2, 1023));
