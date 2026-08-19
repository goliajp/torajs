// Annex B §B.2.4.1 — RegExp.prototype.compile re-initializes the
// receiver in place: flags re-canonicalize through the getter's
// spec order, source updates, lastIndex resets on success but
// stays untouched when a coercion throw fires before
// RegExpInitialize, and a rejected parse is the same catchable
// SyntaxError as `new RegExp(...)`.
let re = /(?:)/;
re.compile("(?:)", "imsuyg");
console.log(re.flags, re.source);

let r2 = /x(y)?/g;
r2.lastIndex = 99;
let bad: any = { toString: function () { throw new Error("boom"); } };
try {
  r2.compile(bad);
} catch (e: any) {
  console.log("caught", e.message, r2.lastIndex, r2.source);
}
try {
  r2.compile("", bad);
} catch (e: any) {
  console.log("caught2", e.message, r2.lastIndex);
}
try {
  r2.compile("(?<x>a)(?<x>b)");
} catch (e: any) {
  console.log("syntax", e instanceof SyntaxError, r2.lastIndex);
}
r2.compile("a(b)c", "g");
console.log(r2.source, r2.flags, r2.lastIndex);
console.log(r2.test("xabcx"), "abc-abc".replace(r2, "_"));
let r3 = /q/;
console.log(r2.compile(r3) === r2, r2.source, r2.flags);
try {
  r2.compile(/z/i, "g");
} catch (e: any) {
  console.log("donor-flags", e instanceof TypeError);
}
