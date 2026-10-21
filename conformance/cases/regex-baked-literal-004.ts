// Literal regexes through the string surface: match, matchAll, replace
// with a callback, split, search, and lookaround programs whose
// sub-programs ride in the build-time image.

const s = "alpha=1;beta=22;gamma=333";
console.log(s.match(/\d+/g)!.join(","));
console.log(s.search(/beta/), s.search(/delta/));
console.log(s.split(/[=;]/).join("|"));
console.log(s.split(/(;)/).length);
for (const m of s.matchAll(/(?<k>[a-z]+)=(?<v>\d+)/g)) {
  console.log(m.index, m.groups!.k, m.groups!.v);
}
console.log(
  s.replace(/(\w+)=(\d+)/g, (_all: string, k: string, v: string) => `${v}:${k}`),
);
console.log("price: 30 USD, 45 EUR".replace(/\d+(?= EUR)/, "X"));
console.log("price: 30 USD, 45 EUR".replace(/(?<!\d)\d(?!\d)/g, "#"));
console.log("aaa bbb".replace(/(?<=a)a/g, "-"));
console.log("abcabc".replace(/(?!b)\w/g, "."));
console.log("The Quick Brown".replace(/\b\w/g, (c: string) => c.toLowerCase()));
console.log("one  two   three".split(/\s+/).length);
console.log("x1y2z3".match(/[a-z]\d/g)!.length);
console.log(/^(a+)+$/.test("aaaa"), /^(?:ab|a)*c$/.test("ababac"));
console.log("Hello".replace(/l+/, "L"), "Hello".replace(/L+/i, "_"));
