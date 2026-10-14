// §12.9.3 — a NumericLiteralSeparator sits between two digits of one
// digit run, and a literal that starts at its `.` follows the same
// rule as one that starts with a digit.
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval(".0_e1")), caught(() => eval(".0_")), caught(() => eval(".0__1")), caught(() => eval(".5e_1")));
console.log(caught(() => eval("1.0_e1")));
console.log(.1_2, .5e1_0, .0_1e2);
