// RegExp v flag — chunk B2: \q{…} ClassStringDisjunction.
// Classes carrying multi-cp strings desugar to an Alt (descending
// string length, then the cp class, then the empty branch); set
// algebra runs componentwise on (cps, strings).

// basic string membership
console.log(/^[\q{abc}]$/v.test("abc"), /^[\q{abc}]$/v.test("ab"));
console.log(
  /^[\q{a|bc|def}]$/v.test("def"),
  /^[\q{a|bc|def}]$/v.test("a"),
  /^[\q{a|bc|def}]$/v.test("x")
);
// longest string wins
const m = "abc".match(/[\q{a|ab|abc}]/v);
console.log(m ? m[0] : null);
// empty alternative matches the empty string
console.log(/^[\q{}]$/v.test(""), /^[\q{ab|}]?$/v.test(""));
// union with a cp class
console.log(/^[[0-9]\q{ab}]$/v.test("5"), /^[[0-9]\q{ab}]$/v.test("ab"));
// intersection / difference over strings
console.log(/^[\q{ab|cd}&&\q{cd|ef}]$/v.test("cd"), /^[\q{ab|cd}&&\q{cd|ef}]$/v.test("ab"));
console.log(/^[\q{ab|cd}--\q{cd}]$/v.test("ab"), /^[\q{ab|cd}--\q{cd}]$/v.test("cd"));
// multi-cp sequence (flag emoji is two code points)
console.log(/^[\q{🇧🇪|abc}]$/v.test("🇧🇪"));
// quantifier over a strings class re-picks per iteration
console.log(/^[\q{ab|c}]+$/v.test("abcab"), /^[\q{ab|c}]+$/v.test("abx"));
// escapes inside \q
console.log(/^[\q{a\-b}]$/v.test("a-b"));
// global scan
const g = "ab-cd".match(/[\q{ab|cd}]/gv);
console.log(g ? g.length : 0);
