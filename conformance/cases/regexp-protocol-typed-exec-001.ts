// A program that replaces RegExp.prototype.exec: the typed String methods
// taking a RegExp (replace, replaceAll, match, search, split) reach it through
// the pattern's protocol method (§22.2.6.x → §22.2.7.1 RegExpExec), and a
// string pattern stays a substring search.
let calls = 0;
const orig = RegExp.prototype.exec;
RegExp.prototype.exec = function (this: RegExp, s: string) {
  calls++;
  return orig.call(this, s);
};
const s = "a1b2";
console.log(s.replace(/\d/, "#"), calls);
console.log(s.replace(/\d/g, "#"), calls);
console.log(JSON.stringify(s.match(/\d/g)), calls);
console.log(s.search(/b/), calls);
console.log(JSON.stringify(s.split(/\d/)), calls);
console.log(s.replaceAll(/\d/g, "_"), calls);
const words: string[] = "x-y".split("-");
console.log(words.length, calls);
RegExp.prototype.exec = function (s: string) {
  return null;
};
console.log("xax".replace(/a/, "b"));
console.log("xax".replace(/a/g, "b"));
