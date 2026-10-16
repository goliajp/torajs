// %RegExp.prototype% owns @@match / @@matchAll / @@replace / @@search /
// @@split under symbol keys only: on an any-typed RegExp the String method
// names are not methods (§13.3.6 TypeError, `?.()` answers undefined), the
// symbol-keyed call still runs, and a by-name patch on the prototype wins.
const re: any = /a/;
for (const name of ["split", "match", "matchAll", "replace", "search"]) {
  try {
    re[name]("bab", "x");
    console.log(name, "no throw");
  } catch (e) {
    console.log(name, e instanceof TypeError);
  }
}
try {
  re.split("bab");
  console.log("split no throw");
} catch (e) {
  console.log("split dot", e instanceof TypeError);
}
try {
  re.replace("bab", "x");
  console.log("replace no throw");
} catch (e) {
  console.log("replace dot", e instanceof TypeError);
}
console.log(re.split?.("bab"));
console.log(JSON.stringify(re[Symbol.split]("bab")));
console.log(re[Symbol.replace]("bab", "x"));
console.log(re.test("bab"), typeof re.split, typeof re.exec);
(RegExp.prototype as any).search = function (this: RegExp, s: string) {
  return "patched " + this.source + " " + s;
};
console.log(re.search("bab"));
