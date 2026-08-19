// annexB §B.1.3 — HTML-like comments are comments in the dynamic
// function's script-goal text: `<!--` anywhere on a line, `-->` when
// its line has produced no token yet. The assembled §20.2.1.1 text
// puts a newline before the body and after the params, so a body
// (or params) that is nothing but such a comment parses to an empty
// function.
const a = Function("\n-->");
console.log(typeof a, a());
const b = Function("-->"); // assembly's `{\n` supplies the terminator
console.log(typeof b, b());
const c = Function("<!--");
console.log(typeof c, c());
const d = Function("<!--", "");
console.log(typeof d, d());
const e = Function("\n-->", "");
console.log(typeof e, e());
// mid-line `-->` after real tokens is NOT a comment: decrement + gt.
const f = Function("var i = 2; var j = 0; return i --> j;");
console.log(f());
console.log("done");
