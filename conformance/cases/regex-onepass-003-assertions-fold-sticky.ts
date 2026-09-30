// A one-pass literal's matcher now carries the assertions, the `i`
// fold over Latin-1 and the sticky entry. Every probe prints the
// literal's answer next to the same pattern through `new RegExp`,
// which runs the runtime engine; the probes the engine answers short
// of the spec (a CR or U+2028 as a line terminator, case folding past
// ASCII, the `iu` word characters) are printed for the literal only.

function shape(re: RegExp, s: string): void {
  const m = re.exec(s);
  if (m === null) console.log(null, re.lastIndex);
  else console.log(m.index, m[0].length, m.length, re.lastIndex);
}

function pair(a: RegExp, b: RegExp, s: string): void {
  console.log(a.exec(s), a.lastIndex);
  console.log(b.exec(s), b.lastIndex);
}

function walk(a: RegExp, s: string): void {
  let m = a.exec(s);
  while (m !== null) {
    console.log(m.index, m[0].length, m, a.lastIndex);
    if (m[0] === "") a.lastIndex = a.lastIndex + 1;
    m = a.exec(s);
  }
  console.log("end", a.lastIndex);
}

// ^ and $ without and with m
pair(/^(\w+)/, new RegExp("^(\\w+)"), "hello world");
pair(/^(\w+)/, new RegExp("^(\\w+)"), " hello");
pair(/(\w+)$/, new RegExp("(\\w+)$"), "hello world");
pair(/^(\w+)$/, new RegExp("^(\\w+)$"), "word");
pair(/^(\w+)$/, new RegExp("^(\\w+)$"), "two words");
pair(/^line/m, new RegExp("^line", "m"), "pre\nline-text\nend");
pair(/^(\w)/m, new RegExp("^(\\w)", "m"), "\nb");
pair(/(\w)$/m, new RegExp("(\\w)$", "m"), "ab\ncd\n");
pair(/^$/m, new RegExp("^$", "m"), "a\n\nb");
walk(/^(\w+)$/gm, "aa\nbbb\n\nc");
walk(new RegExp("^(\\w+)$", "gm"), "aa\nbbb\n\nc");
walk(/^/gm, "a\nb");
walk(new RegExp("^", "gm"), "a\nb");
walk(/$/g, "ab");
walk(new RegExp("$", "g"), "ab");
pair(/(?:^|x)(a)/, new RegExp("(?:^|x)(a)"), "bxa");
pair(/(a)(?:$|;)/, new RegExp("(a)(?:$|;)"), "aa;");
pair(/(^)?(a)/, new RegExp("(^)?(a)"), "ba");
pair(/^(\w+)/, new RegExp("^(\\w+)"), "é ab");
pair(/(.)$/, new RegExp("(.)$"), "ab中");
pair(/(.)$/u, new RegExp("(.)$", "u"), "ab😀");

// \b and \B
pair(/\bword\b/, new RegExp("\\bword\\b"), "a word b xword wordy");
pair(/\bword\b/, new RegExp("\\bword\\b"), "xword wordy");
pair(/\b(\w+)\b/, new RegExp("\\b(\\w+)\\b"), "  ab-cd");
pair(/\B(\w)/, new RegExp("\\B(\\w)"), "abc d");
pair(/(a)\b/, new RegExp("(a)\\b"), "ab a");
pair(/\b(\w+)/, new RegExp("\\b(\\w+)"), "éab");
pair(/\b(\w+)/, new RegExp("\\b(\\w+)"), "中ab");
walk(/\b\w+\b/g, "one two, three");
walk(new RegExp("\\b\\w+\\b", "g"), "one two, three");
walk(/\b/g, "ab cd");
walk(new RegExp("\\b", "g"), "ab cd");
walk(/\B/g, "ab c");
walk(new RegExp("\\B", "g"), "ab c");

// the engine takes `\n` alone as a line terminator; the literal lane
// follows §12.3, so these have no runtime twin
shape(/^(b)/m, "a\rb");
shape(/(a)$/m, "a\rb");
shape(/^(b)/m, "a b");
shape(/^(b)/m, "a b");
shape(/a(.)c/, "a\rc");
shape(/a(.)c/, "a c");
shape(/a(.)c/s, "a c");

// i over Latin-1 (the engine folds ASCII only; literal alone)
shape(/([é])/i, "xÉ");
shape(/([^é])/i, "Ééx");
shape(/([é-ÿ]+)/i, "ÉÏÿŸz");
shape(/([µ])/i, "Μ");
shape(/([µ])/i, "μ");
shape(/([µ])/iu, "Μ");
shape(/([ÿ])/i, "Ÿ");
shape(/([ß])/i, "ẞ");
shape(/([ß])/iu, "ẞ");
shape(/(s)/i, "ſ");
shape(/(s)/iu, "ſ");
shape(/(k)/iu, "K");
shape(/([^k])/iu, "K");
shape(/(\w)/iu, "K");
shape(/(\w)/i, "K");
shape(/\b(.)/iu, " ſ");
shape(/\b(.)/i, " ſ");
// ASCII folding has a twin
pair(/(He)(l+)o/i, new RegExp("(He)(l+)o", "i"), "xHELLo hello");
pair(/([a-c]+)([x-z]*)/i, new RegExp("([a-c]+)([x-z]*)", "i"), "AbCxYz.");
pair(/([^a-c]+)/i, new RegExp("([^a-c]+)", "i"), "xABcdC");
pair(/(a)/i, new RegExp("(a)", "i"), "ÁáA");

// sticky: anchored at lastIndex, the end stored, a miss resets. The
// runtime twin is built through a call, so it is a RegExp of its own.
function dyn(p: string, f: string): RegExp {
  return new RegExp(p, f);
}
const s1 = /(\d+)/y;
const s2 = dyn("(\\d+)", "y");
for (const at of [0, 2, 3, 9]) {
  s1.lastIndex = at;
  s2.lastIndex = at;
  pair(s1, s2, "ab12");
}
walk(/(\w)/y, "ab c");
walk(dyn("(\\w)", "y"), "ab c");
walk(/(\w)/gy, "ab c");
walk(dyn("(\\w)", "gy"), "ab c");
walk(/(a*)/y, "aab");
walk(dyn("(a*)", "y"), "aab");
const s3 = /^(a)/y;
const s4 = dyn("^(a)", "y");
s3.lastIndex = 1;
s4.lastIndex = 1;
pair(s3, s4, "aa");
const s5 = /(.)/uy;
const s6 = dyn("(.)", "uy");
s5.lastIndex = 1;
s6.lastIndex = 1;
shape(s5, "😀x");
shape(s6, "😀x");
const s7 = /(\w)/y;
const s8 = dyn("(\\w)", "y");
s7.lastIndex = 1;
s8.lastIndex = 1;
pair(s7, s8, "é ab");

// the replace walk with a sticky and an anchored pattern
console.log("ab cd".replace(/^(\w+)/, (m: string, w: string) => "<" + w + ">"));
console.log("ab cd".replace(new RegExp("^(\\w+)"), (m: string, w: string) => "<" + w + ">"));
console.log("ab cd".replace(/\b(\w)/g, (m: string, w: string) => w.toUpperCase()));
console.log("ab cd".replace(new RegExp("\\b(\\w)", "g"), (m: string, w: string) => w.toUpperCase()));
console.log("aab".replace(/(a)/gy, (m: string, w: string) => "[" + w + "]"));
console.log("aab".replace(dyn("(a)", "gy"), (m: string, w: string) => "[" + w + "]"));
const y1 = /(a)/y;
y1.lastIndex = 1;
console.log("aab".replace(y1, (m: string, w: string) => "[" + w + "]"), y1.lastIndex);
const y2 = dyn("(a)", "y");
y2.lastIndex = 1;
console.log("aab".replace(y2, (m: string, w: string) => "[" + w + "]"), y2.lastIndex);
console.log("a\nb\nc".replace(/^(\w)$/gm, (m: string, w: string) => w + w));
console.log("a\nb\nc".replace(new RegExp("^(\\w)$", "gm"), (m: string, w: string) => w + w));
console.log("HeLLo hello".replace(/(hel)/gi, (m: string, w: string) => "<" + w + ">"));
console.log("HeLLo hello".replace(new RegExp("(hel)", "gi"), (m: string, w: string) => "<" + w + ">"));
