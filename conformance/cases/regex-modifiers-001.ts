// ES 2025 regexp-modifiers `(?ims-ims:…)` — inline flag groups.
// Effective i/m/s resolve per atom at parse time (merged with the
// global flags) and bake into Inst.pad; the VM and DFA read them per
// instruction instead of from one global flag word.

// add ignoreCase — scoped to the group
console.log(/(?i:a)b/.test("Ab"), /(?i:a)b/.test("AB"));
// remove ignoreCase under global i
console.log(/(?-i:a)b/i.test("aB"), /(?-i:a)b/i.test("Ab"));
// add + remove combined
const re1 = /(?m-i:^a$)/i;
console.log(re1.test("A\n"), re1.test("a\n"));
// dotAll scoped
console.log(/a(?s:.)b/.test("a\nb"), /(?s:.)/.test("\n"), /./.test("\n"));
// multiline scoped
console.log(/(?m:^b)/.test("a\nb"), /^b/.test("a\nb"), /(?-m:^b)/m.test("a\nb"));
// nesting restores outer scope
console.log(/(?i:x(?-i:y)z)/.test("XyZ"), /(?i:x(?-i:y)z)/.test("XYZ"));
// backreference under scoped i
console.log(/(?i:(a)\1)/.test("aA"));
// class + escapes under scoped i
console.log(/(?i:[ab])/.test("A"), /(?i:\x61)/.test("A"));
// negated class canonicalizes before negation
console.log(/[^ab]/i.test("A"), /[^ab]/i.test("c"), /(?i:[^ab])/.test("A"));
// flags properties unaffected by modifier groups
const re2 = /(?s:.)/;
console.log(re2.dotAll, re2.flags === "");
// dynamic construction
console.log(new RegExp("(?i:a)").test("A"));
// global scan interplay
const m = "aA".match(/(?i:a)/g);
console.log(m ? m.length : 0);
// capture extraction through the modifier group (DFA second pass)
const c = "xAby".match(/(?i:(a)(b))/);
console.log(c ? c[1] + "|" + c[2] : null);
// VM-served shapes (lookahead / backref) under i
console.log(/[a-z](?=1)/i.test("A1"), /([a-z])\1/i.test("Aa"));
