// 557-04 — a negated class accepts a lone surrogate. A JS string is a
// sequence of UTF-16 code units, so "\uD83D" is a value; `/./u` and a
// literal `/\uD83D/u` already matched it, `[^]` / `[^a]` / `\S` did not
// (the byte expansion carved D800..DFFF out of the complement).
const hi = "\uD83D";
const lo = "\uDC38";
const pair = "😀";

console.log(JSON.stringify(/[^]/u.exec(hi)));
console.log(JSON.stringify(/[^]/.exec(hi)));
console.log(JSON.stringify(/[^]/u.exec(lo)));
console.log(JSON.stringify(/[^a]/u.exec(hi)));
console.log(JSON.stringify(/[^a]/.exec(lo)));
console.log(JSON.stringify(/\S/u.exec(hi)));
console.log(JSON.stringify(/\S/.exec(lo)));
console.log(JSON.stringify(/\W/u.exec(hi)));
console.log(JSON.stringify(/\D/u.exec(lo)));
console.log(JSON.stringify(/[^a]/iu.exec("A" + hi)));
console.log(JSON.stringify(/[^\p{L}]/u.exec("ab" + hi + "cd")));

// every code unit in the string is one match of [^]
console.log(("a" + hi + "b" + lo + "c").match(/[^]/gu)!.length);
console.log(("x" + hi + "y" + lo).match(/[^x]/g)!.length);
// a proper pair is one code point under u
console.log(JSON.stringify(pair.match(/[^]/gu)));
console.log(JSON.stringify(/[^]{2}/u.exec(hi + lo)));
console.log(JSON.stringify(/^[^]$/u.test(hi)), JSON.stringify(/^[^]$/u.test(pair)));
console.log(JSON.stringify(/[^\uD83D]/u.exec(hi + "z")));
console.log(JSON.stringify(/[^\uD83D]/u.exec(hi)));
console.log(JSON.stringify(("q" + hi + "q").replace(/[^q]/gu, "_")));
console.log(JSON.stringify(("q" + hi + "q").split(/[^q]/u)));
