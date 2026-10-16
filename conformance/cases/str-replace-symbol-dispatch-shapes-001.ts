// §22.1.3.19 step 2 / §22.1.3.20 step 2.c — replace and replaceAll
// hand a searchValue carrying a user @@replace «O, replaceValue»
// whatever the replaceValue is (string, number, function, any) and
// however many trailing arguments follow. Without one, the fallback
// searches for ToString(searchValue) literally and takes step 5's
// IsCallable / step 6's ToString on the replaceValue.

var o: any = {};
o[Symbol.replace] = function (x: string, r: any) {
  return "R:" + x + ":" + typeof r + ":" + (typeof r === "function" ? r("q") : r);
};

var fnRepl: any = function (m: string) {
  return "<" + m + ">";
};
console.log("abc".replace(o, "Z", 1));
console.log("abc".replace(o, 5));
console.log("abc".replace(o, true));
console.log("abc".replace(o, fnRepl));
console.log("abc".replaceAll(o, "Z"));
console.log("abc".replaceAll(o, (m: string) => m + m));
console.log("abc".replaceAll(o, 3, "extra"));
console.log("abc".replaceAll(o));

// a nullish @@replace falls back to the literal search
var lit: any = {};
lit[Symbol.replace] = null;
lit.toString = function () {
  return "b";
};
console.log("abcb".replace(lit, 9), "abcb".replaceAll(lit, false));
console.log("abcb".replace(lit, fnRepl), "abcb".replaceAll(lit, fnRepl));
var strRepl: any = "$&$&";
console.log("abcb".replaceAll(lit, strRepl), "abcb".replace(lit, "-", 0));
