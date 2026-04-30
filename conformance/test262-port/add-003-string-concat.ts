// Adapted from test262: language/expressions/addition/S11.6.1_A4_T2.js
// Spec: when both operands are strings, `+` is concatenation.
function check(): number {
  if ("a" + "b" !== "ab") { throw "#1"; }
  if ("hello" + " " + "world" !== "hello world") { throw "#2"; }
  let s: string = "x";
  let t: string = "y";
  if (s + t !== "xy") { throw "#3"; }
  if (s + s !== "xx") { throw "#4"; }
  return 0;
}
console.log(check());
