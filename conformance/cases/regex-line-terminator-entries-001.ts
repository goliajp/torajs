// a dot without s skips all four line terminators (\n \r u+2028
// u+2029), and a multiline ^ / $ sits next to any of them, on every
// entry: match, test, search, split, matchAll, replace with a string
// and with a callback, for a literal, a literal with a group, and a
// pattern built at run time.
function lit(src: string, flags: string): RegExp[] {
  return [new RegExp(src, flags), new RegExp("(?:" + src + ")", flags + "g")];
}

const patterns: RegExp[][] = [
  [/a.+c/, /(a).+c/, ...lit("a.+c", "")],
  [/a.c/, /(a).c/, ...lit("a.c", "")],
  [/a.c/s, /(a).c/s, ...lit("a.c", "s")],
  [/^b/m, /^(b)/m, ...lit("^b", "m")],
  [/a$/m, /(a)$/m, ...lit("a$", "m")],
  [/x\n^b/m, /(x)\r^b/m, ...lit("[\\r\\u2028]^b", "m")],
  [/a.c/u, /(a).c/u, ...lit("a.c", "u")],
];

const subjects = [
  "a\rc",
  "a\u2028c",
  "a\u2029c",
  "a\nc",
  "a\rb",
  "a\u2028b",
  "x\rb",
  "x\u2028b",
  "abc",
  "\u00e9a\rc \u00e9a\u00e8c",
  "\u4e2da\u2028c a\u4e2dc",
  "\u4e2da\u2029b\na",
  "\u00e9\u00e8b",
];

function show(m: RegExpMatchArray | null): string {
  return m === null ? "null" : m.index + ":" + m[0].length;
}

for (let p = 0; p < patterns.length; p = p + 1) {
  for (let k = 0; k < patterns[p].length; k = k + 1) {
    const re = patterns[p][k];
    const row: string[] = [];
    for (let i = 0; i < subjects.length; i = i + 1) {
      const s = subjects[i];
      re.lastIndex = 0;
      const t = re.test(s);
      re.lastIndex = 0;
      const sr = s.search(re);
      re.lastIndex = 0;
      const m = show(s.match(re));
      const sp = s.split(re).length;
      const rs = s.replace(re, "#").length;
      const rf = s.replace(re, (w: string) => "<" + w.length + ">");
      const all = re.global ? [...s.matchAll(re)].length : -1;
      row.push(t + "/" + sr + "/" + m + "/" + sp + "/" + rs + "/" + rf.length + "/" + all);
    }
    console.log(p, k, row.join(" "));
  }
}
