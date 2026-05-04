// Phase 1a: regex edge cases — lazy quantifier, negated class,
// multiline / dotall flags, nested groups, escaped specials,
// hex escape.

const r1 = /a.*?b/;
console.log(r1.test("axxxxxb"));
console.log(r1.test("ab"));

const r2 = /[^abc]/;
console.log(r2.test("a"));
console.log(r2.test("d"));

const r3 = /^line2/m;
console.log(r3.test("line1\nline2\nline3"));
console.log(r3.test("line1\nlineX"));

const r4 = /a.b/s;
console.log(r4.test("a\nb"));
const r5 = /a.b/;
console.log(r5.test("a\nb"));

const r6 = /((ab)+)c/;
console.log(r6.test("ababc"));
console.log(r6.test("ababx"));

const r7 = /\.\*\+/;
console.log(r7.test(".*+"));
console.log(r7.test("abc"));

const r8 = /\x41/;
console.log(r8.test("A"));
console.log(r8.test("a"));
