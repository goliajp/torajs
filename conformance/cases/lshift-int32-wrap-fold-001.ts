// §13.9 int32-wrap left shift under branch folding — the shifted
// value leaves i32 range and wraps; branch_fold's interval model
// must not fold the comparison with exact-multiplication semantics
// (test262 left-shift S11.7.1 A4/A5 appeared cluster).
console.log(-2147483648 << 1);
if (-2147483648 << 1 !== 0) {
  console.log("wrong-branch");
} else {
  console.log("fold-ok");
}
let x = -2147483648;
if (x << 1 !== 0) {
  console.log("wrong-var-branch");
} else {
  console.log("var-fold-ok");
}
console.log(1073741824 << 1);
console.log((1 << 30) << 2);
console.log(-8 << 3);
