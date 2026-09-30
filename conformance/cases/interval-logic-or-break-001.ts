// a digit loop whose break is a short-circuit `||`: the body sees the
// code unit inside [48, 57], so the accumulator runs as an integer
// and must leave that lane exactly where Number arithmetic rounds
function parseDigits(src: string): number {
  let i = 0;
  let v = 0;
  while (i < src.length) {
    const d = src.charCodeAt(i);
    if (d < 48 || d > 57) break;
    v = v * 10 + (d - 48);
    i = i + 1;
  }
  return v;
}
console.log(parseDigits("0"));
console.log(parseDigits("42"));
console.log(parseDigits("123abc"));
console.log(parseDigits("abc"));
console.log(parseDigits(""));
// exact in f64 and in i64
console.log(parseDigits("9007199254740992"));
// past 2^53: f64 rounds to even
console.log(parseDigits("9007199254740993"));
console.log(parseDigits("9007199254740995"));
console.log(parseDigits("18014398509481985"));
// past 2^63: no i64 holds it
console.log(parseDigits("9223372036854775807"));
console.log(parseDigits("9223372036854775808"));
console.log(parseDigits("18446744073709551616"));
console.log(parseDigits("99999999999999999999"));
console.log(parseDigits("12345678901234567890123456789"));
// the two-byte string lane
console.log(parseDigits("2718é"));
console.log(parseDigits("é2718"));
console.log(parseDigits("99999999999999999999日"));

// the same loop over many numbers in one string
function sumNumbers(src: string): number {
  let i = 0;
  let sum = 0;
  while (i < src.length) {
    const c = src.charCodeAt(i);
    if (c >= 48 && c <= 57) {
      let v = 0;
      while (i < src.length) {
        const d = src.charCodeAt(i);
        if (d < 48 || d > 57) break;
        v = v * 10 + (d - 48);
        i = i + 1;
      }
      sum = sum + v;
    } else {
      i = i + 1;
    }
  }
  return sum;
}
console.log(sumNumbers("1 2 3 40 500"));
console.log(sumNumbers("9007199254740993 x 9007199254740993"));
console.log(sumNumbers("99999999999999999999,1"));
console.log(sumNumbers("no digits here"));
