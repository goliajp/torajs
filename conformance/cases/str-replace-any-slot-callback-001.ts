// The callback half of the `any`-slot pattern seam. §22.1.3.19 step 2
// hands a searchValue carrying `@@replace` — every RegExp does — to
// that method, whatever the replaceValue is; the typed lane could only
// route a function replacer when it could see the pattern was a regex
// AT COMPILE TIME, so `var re = /b/; s.replace(re, function (m) { … })`
// did not compile at all.
//
// How many capture arguments to build is the callback's own declared
// count here, not the pattern's: a pattern known only as `any` cannot
// be counted statically, and a slot past the pattern's real group
// count reads back as the non-participating sentinel — the same
// `undefined` §22.2.6.11 step 14.g would pass there anyway.
//
// Every declared parameter has to be a plain `string` for that to
// hold, which is why the `(position, string)` tail and a promoted
// `function () { …this… }` keep their loud route: the tail pins every
// argument before it, and a receiver parameter would be miscounted as
// the match.

var re: any = /b/;
var g: any = /(b)(c)?/g;
var lit: any = "b";

console.log("one", "abcb".replace(re, function (m: string) {
  return "<" + m + ">";
}));

// two captures, the second non-participating on the last match
console.log("caps", "abcb".replace(g, function (m: string, p1: string, p2: string) {
  return "[" + p1 + p2 + "]";
}));

// declaring nothing is legal, and the count is still the callback's
console.log("bare", "abcb".replace(g, function () {
  return "Z";
}));

console.log("all", "abcb".replaceAll(g, function () {
  return "#";
}));

// the same `any` slot holding a STRING takes the literal-needle leg,
// whose callback rides the boxed entry (§22.1.3.18 step 10)
console.log("literal", "abcb".replace(lit, function (m: string) {
  return "L";
}), "abcb".replaceAll(lit, function (m: string) {
  return "L";
}));

// §22.1.3.20 step 2.b still rejects a non-global RegExp first
try {
  console.log("no-throw", "abcb".replaceAll(re, function (m: string) {
    return "N";
  }));
} catch (e: any) {
  console.log("replaceAll throws", e instanceof TypeError);
}

// the statically-typed spelling is untouched
console.log("typed", "abcb".replace(/(b)/g, function (m: string, p1: string) {
  return "(" + p1 + ")";
}));
