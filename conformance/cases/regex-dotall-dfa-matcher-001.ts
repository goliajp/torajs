// capture-free literals that are not one-pass (a dot run that must
// give back its tail, overlapping alternatives) walk their compiled
// leftmost-first matcher. under s a dot takes every line terminator,
// without it none of the four; under u a surrogate pair is one dot.
const greedy = /a.+c/s;
const star = /a.*c/s;
const lazy = /a.+?c/s;
const lazyStar = /a.*?c/s;
const plain = /a.+c/;
const any = /.+/s;
const anyU = /a.+c/su;
const lazyU = /a.+?c/su;
const one = /a.c/u;
const two = /a..c/;
const alt = /a|ab/;
const digits = /\d|\d\d/;
const glob = /a.+?c/gs;
const sticky = /a.+c/sy;

function len(s: string, re: RegExp): number {
  const m = s.match(re);
  return m !== null ? m[0].length : -1;
}

function show(s: string): string {
  return [
    len(s, greedy),
    len(s, star),
    len(s, lazy),
    len(s, lazyStar),
    len(s, plain),
    len(s, any),
    len(s, anyU),
    len(s, lazyU),
    len(s, one),
    len(s, two),
  ].join(",");
}

const subjects = [
  "pre a\nmiddle\nc post 17",
  "a\rc",
  "a\r\nc",
  "a\u2028c",
  "a\u2029c a\u2028\u2029c",
  "ac abc",
  "zzz no match here",
  "a",
  "",
  "xa ya za\nc wc",
  "caaab cbc",
  "a\u00e9\u00ffc \u00e0",
  "a\u4e2dc",
  "a\ud83d\ude00c",
  "\ud83d\ude00 a\ud83d\ude00\u4e2dc\nz\ud83d\ude00c",
  "a\ud83dc",
];
for (let i = 0; i < subjects.length; i = i + 1) console.log(i, show(subjects[i]));

// where the match starts, and what the other entries answer
for (const s of subjects) {
  const m = greedy.exec(s);
  const n = lazyU.exec(s);
  console.log(
    m === null ? -1 : m.index,
    m === null ? -1 : m[0].charCodeAt(m[0].length - 2),
    n === null ? -1 : n.index,
    lazy.test(s),
    s.search(star),
  );
}

console.log(len("ab", alt), len("x12 3", digits), len("abab", /(?:ab)*a/));
console.log("xa1c a2c\na3c".replace(glob, "[$&]"));
console.log(JSON.stringify("a1c-a\n2c-q".split(lazy)));
console.log([..."a1c a\r2c a3".matchAll(glob)].map((m) => m.index + ":" + m[0].length).join(" "));
glob.lastIndex = 0;
let hits = "";
let g = glob.exec("a1ca2c a3c");
while (g !== null) {
  hits = hits === "" ? String(glob.lastIndex) : hits + "," + String(glob.lastIndex);
  g = glob.exec("a1ca2c a3c");
}
console.log(hits, glob.lastIndex);
sticky.lastIndex = 1;
console.log(sticky.test("xa\nc"), sticky.lastIndex, sticky.test("xa\nc"), sticky.lastIndex);

// a haystack built at run time, long enough for many candidate starts
let long = "";
for (let i = 0; i < 40; i = i + 1) long = long + "a" + String.fromCharCode(10 + (i % 4)) + i.toString();
console.log(len(long, greedy), len(long + "c", greedy), len(long + "c", lazy), len(long, plain));

// empty matches step by one unit, or one code point under u
console.log("aaa".replace(/a*?/g, "-"));
console.log([..."\ud83d\ude00b".matchAll(/a*?/gu)].map((m) => m.index).join(","));
// word boundaries, folding and stickiness on a subset walk
console.log(len("ab1 cd22 x9", /\b\w+\d/), len("x.cd22", /\b\w+\d/), len("ABABa aB", /(?:ab)*a/i));
const stickyPlain = /a.+c/y;
stickyPlain.lastIndex = 1;
console.log(stickyPlain.test("xabc"), stickyPlain.lastIndex, stickyPlain.test("xa\rc"), stickyPlain.lastIndex);
// past the state budget the literal keeps the runtime path
const wide = /(?:a|b)*a(?:a|b){9}/;
console.log(len("bbabababababb", wide), len("ab" + "a".repeat(9) + "c", wide), len("abababab", wide));
