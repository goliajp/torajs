// Phase 1a: literal RegExp + .test() basic surface.
// Spans literal char, anchors, char classes, quantifiers,
// alternation, case-insensitive flag, word boundary.

const re1 = /hello/;
console.log(re1.test("hello world"));
console.log(re1.test("goodbye"));

const re2 = /^abc$/;
console.log(re2.test("abc"));
console.log(re2.test("abcd"));

const re3 = /[0-9]+/;
console.log(re3.test("abc123"));
console.log(re3.test("nothing"));

const re4 = /Hello/i;
console.log(re4.test("hello"));
console.log(re4.test("HELLO"));

const re5 = /a(b|c)d/;
console.log(re5.test("abd"));
console.log(re5.test("acd"));
console.log(re5.test("aed"));

const re6 = /\d{2,4}/;
console.log(re6.test("12"));
console.log(re6.test("1"));

const re7 = /\bword\b/;
console.log(re7.test("a word here"));
console.log(re7.test("password"));
