// `s.replace(re, text)` / `s.replaceAll(re, text)` with a string
// replacement, for literals whose matcher is synthesized: every `$`
// form, no match, matches at either end, empty matches, more hits than
// the splice records on the stack, Latin-1 and UTF-16 subjects and
// replacements, `g`, `y` and neither. Each pattern also runs through
// `new RegExp` over a source only known at run time, which has no
// synthesized matcher.

// empty at run time: keeps the source of `rt` out of reach of the compiler
const pad = process.argv.slice(99).join("");
function rt(p: string, f: string): RegExp {
  return new RegExp(p + pad, f);
}

function both(s: string, a: RegExp, b: RegExp, r: string): void {
  console.log(s.replace(a, r) + " | " + s.replace(b, r));
}

const subject = "abbc xxx abc yyy abbbbc";
for (const r of ["XY", "<$1>", "[$&]", "$$", "$$1", "($`)", "($')", "$0$2$x$", "$01$10", "", "$"]) {
  both(subject, /a(b+)c/g, rt("a(b+)c", "g"), r);
  both(subject, /a(b+)c/, rt("a(b+)c", ""), r);
}

// no match, match at the start / end / whole subject
both("xbbc xxx", /a(b+)c/g, rt("a(b+)c", "g"), "Z");
both("abc tail", /a(b+)c/g, rt("a(b+)c", "g"), "Z");
both("head abbc", /a(b+)c/g, rt("a(b+)c", "g"), "Z");
both("abbbc", /a(b+)c/g, rt("a(b+)c", "g"), "<$1>");
both("", /a(b+)c/g, rt("a(b+)c", "g"), "Z");
both("aaaa", /a(b+)c/g, rt("a(b+)c", "g"), "Z");

// optional group: an unset group substitutes nothing
both("ac abc", /a(b)?c/g, rt("a(b)?c", "g"), "[$1]");

// named groups keep `$<name>`
both("k=v", /(?<k>\w)=(?<v>\w)/, rt("(?<k>\\w)=(?<v>\\w)", ""), "$<v>=$<k>");

// empty matches
both("abc", /x*/g, rt("x*", "g"), "-");
both("abc", /x*/, rt("x*", ""), "-");
both("a😀b", /x*/gu, rt("x*", "gu"), "-");
// without `u` an empty match steps into the pair
console.log("a😀b".replace(/x*/g, "-"));

// more hits than the stack record
const many = "ab".repeat(40);
both(many, /a/g, rt("a", "g"), "_");
both(many, /a/g, rt("a", "g"), "$&$&");

// Latin-1 subject past ASCII
both("aéébé", /é+/g, rt("é+", "g"), "[$&]");
both("aéébé", /(é)(b)/g, rt("(é)(b)", "g"), "$2$1");
both("aéébé", /é/g, rt("é", "g"), "日");
both("caféc", /c/g, rt("c", "g"), "Ç");

// UTF-16 subject and replacement
both("x日y日日z", /日+/g, rt("日+", "g"), "<$&>");
both("x日y日日z", /(日+)z/, rt("(日+)z", ""), "$1!");
both("x日y日日z", /日/g, rt("日", "g"), "");
both("日a日", /a/g, rt("a", "g"), "b");
both("a😀😀b😀", /😀+/gu, rt("😀+", "gu"), "[$&]");
both("abc", /b/g, rt("b", "g"), "😀");

// replaceAll
console.log(subject.replaceAll(/a(b+)c/g, "$1") + " | " + subject.replaceAll(rt("a(b+)c", "g"), "$1"));

// lastIndex: g resets it; y reads and stores it
const g = /a(b+)c/g;
const gr = rt("a(b+)c", "g");
g.lastIndex = 5;
gr.lastIndex = 5;
console.log(subject.replace(g, "Z") + " " + g.lastIndex + " | " + subject.replace(gr, "Z") + " " + gr.lastIndex);
const y = /a(b+)c/y;
const yr = rt("a(b+)c", "y");
for (const at of [0, 9, 3, 17, 40]) {
  y.lastIndex = at;
  yr.lastIndex = at;
  const a = subject.replace(y, "<$1>") + " " + y.lastIndex;
  const b = subject.replace(yr, "<$1>") + " " + yr.lastIndex;
  console.log(at + ": " + a + " | " + b);
}
const gy = /a(b+)c ?/gy;
const gyr = rt("a(b+)c ?", "gy");
console.log("abc abbc x abc".replace(gy, "$1") + " " + gy.lastIndex + " | " + "abc abbc x abc".replace(gyr, "$1") + " " + gyr.lastIndex);

// a leading unit behind a zero-width assertion still starts every match
both("ab\nab xab", /^a/gm, rt("^a", "gm"), "<$&>");
both("ab xab-ab", /\ba/g, rt("\\ba", "g"), "<$&>");
// a UTF-16 subject skipped to its ASCII leading unit
both("日ab日xab", /a(b)/g, rt("a(b)", "g"), "[$1]");

// eleven groups: `$11` names the last, `$1` then `1` past it
const eleven = "(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)";
both("xabcdefghijky", /(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)/g, rt(eleven, "g"), "$11$10$1|$01$012");

// sticky under `u` with lastIndex on the second half of a pair
const uy = /😀?a/uy;
for (const at of [0, 1, 2, 3]) {
  uy.lastIndex = at;
  const r = "😀a😀a".replace(uy, "[$&]");
  console.log(at + ": " + r + " " + uy.lastIndex);
}
