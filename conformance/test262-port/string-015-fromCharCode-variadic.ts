// Adapted from test262 string/static/fromCharCode/* — variadic
// arity. Each numeric arg is a UTF-16 code unit; result is the
// concatenation. tr lowers this as a chain of single-char alloc
// + str_concat (O(n) chain — fine for small string-build use).
function check(): number {
  // 5-arg → "ABCDE".
  if (String.fromCharCode(65, 66, 67, 68, 69) !== "ABCDE") {
    throw "#1: 5-arg";
  }

  // 2-arg → "Hi".
  if (String.fromCharCode(72, 105) !== "Hi") { throw "#2: 2-arg"; }

  // 1-arg still works (single-arg is the original signature, must
  // not regress when the variadic intercept landed).
  if (String.fromCharCode(65) !== "A") { throw "#3: 1-arg"; }

  // 0-arg → empty string.
  if (String.fromCharCode() !== "") { throw "#4: 0-arg"; }

  // Build a longer ASCII run to exercise the concat chain depth.
  // 0x30..0x39 == "0123456789".
  let digits = String.fromCharCode(48, 49, 50, 51, 52, 53, 54, 55, 56, 57);
  if (digits !== "0123456789") { throw "#5: 10-arg"; }

  // Mix lowercase + uppercase.
  if (String.fromCharCode(72, 101, 108, 108, 111) !== "Hello") {
    throw "#6: Hello";
  }

  return 0;
}
console.log(check());
