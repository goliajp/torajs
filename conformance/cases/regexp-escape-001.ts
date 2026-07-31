// RegExp.escape — ES2025 §22.2.5.1
console.log(RegExp.escape("hello"));
console.log(RegExp.escape("a.b*c+d?"));
console.log(RegExp.escape("^$\\.*+?()[]{}|/"));
console.log(RegExp.escape("2fa"));
console.log(RegExp.escape(""));
console.log(RegExp.escape("tab\there"));
console.log(RegExp.escape("sp ace"));
console.log(RegExp.escape("quo'te\"dq"));
console.log(RegExp.escape("uni✓code"));
console.log(RegExp.escape("-=<>#&!%:;@~`,"));
// non-string throws TypeError (no ToString)
try {
  RegExp.escape(1 as any);
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
try {
  RegExp.escape(null as any);
} catch (e) {
  console.log("caught2", e instanceof TypeError);
}
// reflection face
console.log(typeof RegExp.escape, RegExp.escape.length);
