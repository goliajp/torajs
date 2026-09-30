// A literal whose program is one-pass runs a matcher built at compile
// time; the same pattern through `new RegExp` runs the runtime engine.
// Every probe prints both answers, so the two lanes are held to the
// same output: captures, non-participating groups, empty matches, a
// global walk's lastIndex, a miss resetting it, and the patterns the
// lane declines (ambiguous alternation, backreference, anchor, sticky).

function pair(a: RegExp, b: RegExp, s: string): void {
  console.log(a.exec(s), a.lastIndex);
  console.log(b.exec(s), b.lastIndex);
}

function walk(a: RegExp, s: string): void {
  let m = a.exec(s);
  while (m !== null) {
    console.log(m, a.lastIndex);
    if (m[0] === "") a.lastIndex = a.lastIndex + 1;
    m = a.exec(s);
  }
  console.log("end", a.lastIndex);
}

walk(/([a-z]+)=(\d+)/g, "kq=17;=3;abc=905;zz12;");
walk(new RegExp("([a-z]+)=(\\d+)", "g"), "kq=17;=3;abc=905;zz12;");
walk(/([a-z]+)=(\d+)/g, "aaaaaaaa;bbbb=");
walk(new RegExp("([a-z]+)=(\\d+)", "g"), "aaaaaaaa;bbbb=");
walk(/(a*)/g, "baa");
walk(new RegExp("(a*)", "g"), "baa");
walk(/([^=;]+)=([^;]*);?/g, "k=v;=x;ab=;q");
walk(new RegExp("([^=;]+)=([^;]*);?", "g"), "k=v;=x;ab=;q");
walk(/\d+/g, "a1b22c333");
walk(new RegExp("\\d+", "g"), "a1b22c333");

pair(/(a)?(b)/, new RegExp("(a)?(b)"), "abb");
pair(/(a)?(b)/, new RegExp("(a)?(b)"), "b");
pair(/(?:(a)|(b))c/, new RegExp("(?:(a)|(b))c"), "xbc");
pair(/(a|)*b/, new RegExp("(a|)*b"), "aab");
pair(/(He)(l+)o/i, new RegExp("(He)(l+)o", "i"), "xHELLo hello");
pair(/(a.)(.b)?/s, new RegExp("(a.)(.b)?", "s"), "a\n\nb");
pair(/(a.)(.b)?/, new RegExp("(a.)(.b)?"), "a\nab.b");
pair(/(x)(y)(z)(w)/, new RegExp("(x)(y)(z)(w)"), "..xyzw");
pair(/(?:(\d)(\w)?,)+/, new RegExp("(?:(\\d)(\\w)?,)+"), "1a,2,3b,");
pair(/(cat|dog)s?/, new RegExp("(cat|dog)s?"), "hotdogs");
pair(/(a*?)/, new RegExp("(a*?)"), "aa");
pair(/([a-z]+)=(\d+)/, new RegExp("([a-z]+)=(\\d+)"), "");
pair(/(?<k>[a-z]+)=(?<v>\d+)/, new RegExp("(?<k>[a-z]+)=(?<v>\\d+)"), "x=9");

// the lane declines these; both sides are the runtime engine
pair(/(a|ab)(c|bcd)/, new RegExp("(a|ab)(c|bcd)"), "abcd");
pair(/(a)\1/, new RegExp("(a)\\1"), "xaa");
pair(/^(\w+)$/, new RegExp("^(\\w+)$"), "word");
const sticky = /(\d+)/y;
sticky.lastIndex = 2;
console.log(sticky.exec("ab12"), sticky.lastIndex);

// a lastIndex past the end misses and resets
const late = /(a)/g;
late.lastIndex = 9;
console.log(late.exec("aaa"), late.lastIndex);

// the exec-record shape: index, a group's length, a parsed group
const rec = /([a-z]+)=(\d+)/g;
let total = 0;
for (let i = 0; i < 3; i = i + 1) {
  rec.lastIndex = 0;
  let m = rec.exec("a=1;bb=22;ccc=333;");
  while (m !== null) {
    total = total + m.index + m[1].length + parseInt(m[2], 10);
    m = rec.exec("a=1;bb=22;ccc=333;");
  }
}
console.log(total);

// a non-ASCII subject takes the runtime engine on both sides
pair(/([a-z]+):(\d+)/, new RegExp("([a-z]+):(\\d+)"), "é ab:12");
