// Adapted from test262: language/expressions/addition/* — JS coerces
// the Number side to a string when one operand is String. tr routes
// through __torajs_i64_to_str / __torajs_f64_to_str.
function check(): number {
  let n: number = 42;
  if ("answer is " + n !== "answer is 42") { throw "#1"; }
  if (n + " is the answer" !== "42 is the answer") { throw "#2"; }
  if ("a" + 1 + "b" !== "a1b") { throw "#3"; }   // chain
  if ("" + 0 !== "0") { throw "#4"; }
  if ("x" + -7 !== "x-7") { throw "#5"; }
  return 0;
}
console.log(check());
