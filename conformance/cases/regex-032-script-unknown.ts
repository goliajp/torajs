// \p{Script=Unknown} / \p{scx=Unknown}: UAX24 Zzzz — the complement of
// Scripts.txt (unassigned, private use, surrogates). Synthesized by
// labs/ucd-gen/gen.py since the file only lists assigned-script cps.

// private use cp is Unknown
console.log(/\p{Script=Unknown}/u.test("\u{E000}"));
console.log(/\p{Script=Unknown}/u.test("\u{F8FF}"));
// unassigned plane-16 cp
console.log(/\p{scx=Unknown}/u.test("\u{10FFFF}"));
// short-code alias resolves
console.log(/\p{Script=Zzzz}/u.test("\u{E000}"));
console.log(/\p{scx=Zzzz}/u.test("\u{E000}"));
// assigned cps are not Unknown
console.log(/\p{Script=Unknown}/u.test("A"));
console.log(/\p{Script=Unknown}/u.test("汉"));
console.log(/\p{scx=Unknown}/u.test("ـ")); // Arabic tatweel, scx-listed
// negated form
console.log(/\P{Script=Unknown}/u.test("\u{E000}"));
console.log(/\P{Script=Unknown}/u.test("A"));
// replace across mixed content
console.log("x\u{E000}y\u{F8FF}z".replace(/\p{Script=Unknown}/gu, "#"));
// in-class form
console.log(/[\p{Script=Unknown}]/u.test("\u{E000}"));
console.log(/[\p{Script=Unknown}q]/u.test("q"));
