// chunk 804 — ECMA Annex B §B.1.4: a decimal escape past the group
// count reinterprets as a LegacyOctalEscapeSequence (longest octal
// prefix, first digit 0-3 up to 3 digits / 4-7 up to 2) followed by
// literal digits; \8 \9 are identity escapes. u-flag patterns keep
// the loud rejection (spec SyntaxError).
console.log(/(a)\12/.test("a\n"));
console.log(/(a)\9/.test("a9"));
console.log(/(a)\8/.test("a8"));
console.log(/\123/.test("S"));
console.log(/\1234/.test("S4"));
console.log(/\777/.test("?7"));
console.log(/\47/.test("'"));
console.log(/(a)\2/.test("a\x02"));
console.log(/(a)\12/.test("ab"));
console.log(/(a)\1/.test("aa"));
