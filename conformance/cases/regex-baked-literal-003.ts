// `new RegExp(literal)` and `new RegExp(literal, flags)` re-derive from
// the literal's source; Unicode property escapes in a build-time
// compiled literal match through the tables the image carries.

const base = /(?<w>[a-z]+)\d/i;
const same = new RegExp(base);
const gi = new RegExp(base, "gi");
console.log(same.source, same.flags, same === base);
console.log(gi.source, gi.flags);
console.log("Ab1 cd2 EF3".replace(gi, "<$<w>>"));
console.log(same.exec("xx Q9")![0], same.exec("xx Q9")!.groups!.w);

const letters = /\p{L}+/gu;
console.log("héllo wörld ελληνικά 123".match(letters)!.join("|"));
const greek = /\p{Script=Greek}+/u;
console.log(greek.exec("abc αβγ def")![0]);
const notLetter = /\P{L}+/u;
console.log(JSON.stringify(notLetter.exec("abc, 12 def")![0]));
const num = /^\p{N}+$/u;
console.log(num.test("١٢٣"), num.test("12a"));
const mixed = /[\p{Lu}\d]+/u;
console.log(mixed.exec("abcÄÖ12xyz")![0]);
const twoProps = /\p{Lu}\p{Ll}/u;
console.log(twoProps.exec("aBcD")![0]);
const setOps = /[\p{L}--\p{Script=Latin}]+/v;
console.log(setOps.exec("abcжзиdef")![0]);
const either = /\p{L}|\p{N}/gu;
console.log("a-1-ß".match(either)!.join(","));
const cased = /\p{Lu}/giu;
console.log("aBc".match(cased)!.join(","));
const emoji = /\p{Emoji_Presentation}/u;
console.log(emoji.test("ok 😀"), emoji.test("ok"));

const u2 = new RegExp(letters, "gu");
console.log(u2.flags, "ab cd".match(u2)!.length);
