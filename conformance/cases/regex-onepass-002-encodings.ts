// A one-pass literal's matcher walks the subject's own code units:
// Latin-1 beyond ASCII, UTF-16, surrogate pairs. The same pattern
// through `new RegExp` runs the runtime engine over a transcoded
// copy; every probe prints both answers, so the two lanes are held
// to the same captures, indices, lastIndex flow and replace output.

function shape(re: RegExp, s: string): void {
  const m = re.exec(s);
  if (m === null) console.log(null, re.lastIndex);
  else console.log(m.index, m[0].length, m.length, re.lastIndex);
}

function pair(a: RegExp, b: RegExp, s: string): void {
  console.log(a.exec(s), a.lastIndex);
  console.log(b.exec(s), b.lastIndex);
}

// an empty match is stepped over by one unit, or by the pair it
// sits on under `u` (AdvanceStringIndex), so the walk terminates
function walk(a: RegExp, s: string, u: boolean): void {
  let m = a.exec(s);
  while (m !== null) {
    console.log(m.index, m[0].length, m, a.lastIndex);
    if (m[0] === "") {
      const c = s.charCodeAt(a.lastIndex);
      const d = s.charCodeAt(a.lastIndex + 1);
      const pairAt = u && c >= 0xd800 && c <= 0xdbff && d >= 0xdc00 && d <= 0xdfff;
      a.lastIndex = a.lastIndex + (pairAt ? 2 : 1);
    }
    m = a.exec(s);
  }
  console.log("end", a.lastIndex);
}

// Latin-1 subjects
pair(/([a-z]+):(\d+)/, new RegExp("([a-z]+):(\\d+)"), "é ab:12");
pair(/([^:]+):([^;]*)/, new RegExp("([^:]+):([^;]*)"), "éÿ:àç;");
pair(/(\w+)=(.)/, new RegExp("(\\w+)=(.)"), "k=é");
pair(/([é-ÿ]+)/, new RegExp("([é-ÿ]+)"), "aéÿè");
pair(/(.)(.)/, new RegExp("(.)(.)"), "éÿ");
walk(/([a-z]+)=([^;]*);/g, "é=ÿ;ab=cé;=x;", false);
walk(new RegExp("([a-z]+)=([^;]*);", "g"), "é=ÿ;ab=cé;=x;", false);
walk(/(a*)/g, "éaa", false);
walk(new RegExp("(a*)", "g"), "éaa", false);

// UTF-16 subjects
pair(/([a-z]+):(\d+)/, new RegExp("([a-z]+):(\\d+)"), "中 ab:12");
pair(/([^:]+):([^;]*)/, new RegExp("([^:]+):([^;]*)"), "中:日;");
pair(/(\w+)=(.)/, new RegExp("(\\w+)=(.)"), "k=中");
pair(/(\w+)=(.)/u, new RegExp("(\\w+)=(.)", "u"), "k=😀");
shape(/(\w+)=(.)/, "k=😀");
shape(new RegExp("(\\w+)=(.)"), "k=😀");
pair(/(.)(.)/u, new RegExp("(.)(.)", "u"), "😀x");
shape(/(.)(.)/, "😀x");
shape(new RegExp("(.)(.)"), "😀x");
pair(/([^a]+)/u, new RegExp("([^a]+)", "u"), "a中😀b");
pair(/([^a]+)/, new RegExp("([^a]+)"), "a中😀b");
pair(/([é-ÿ]+)/, new RegExp("([é-ÿ]+)"), "中éÿĀ");
walk(/([a-z]+)=([^;]*);/g, "中=😀;a=中;", false);
walk(new RegExp("([a-z]+)=([^;]*);", "g"), "中=😀;a=中;", false);
walk(/(x*)/gu, "😀x", true);
walk(new RegExp("(x*)", "gu"), "😀x", true);
walk(/(x*)/g, "😀x", false);
walk(new RegExp("(x*)", "g"), "😀x", false);

// lone surrogates: one unit, printed by position
const lone = "\ud83d" + "x" + "\ude00";
shape(/(.)x(.)/u, lone);
shape(new RegExp("(.)x(.)", "u"), lone);
shape(/(.)x(.)/, lone);
shape(new RegExp("(.)x(.)"), lone);
shape(/([^x]+)/u, lone);
shape(new RegExp("([^x]+)", "u"), lone);

// a lastIndex inside a pair starts at the pair under `u`
const mid1 = /./gu;
mid1.lastIndex = 1;
shape(mid1, "😀x");
const mid2 = new RegExp(".", "gu");
mid2.lastIndex = 1;
shape(mid2, "😀x");
const mid3 = /./g;
mid3.lastIndex = 1;
shape(mid3, "😀x");
const mid4 = new RegExp(".", "g");
mid4.lastIndex = 1;
shape(mid4, "😀x");

// the replace callback's walk over every encoding
console.log("é=ÿ;ab=cé;=x;".replace(/([a-z]+)=([^;]*);/g, (w: string, k: string, v: string) => v + ":" + k + "|"));
console.log("é=ÿ;ab=cé;=x;".replace(new RegExp("([a-z]+)=([^;]*);", "g"), (w: string, k: string, v: string) => v + ":" + k + "|"));
console.log("中=😀;ab=c中;=x;".replace(/([a-z]+)=([^;]*);/g, (w: string, k: string, v: string) => v + ":" + k + "|"));
console.log("中=😀;ab=c中;=x;".replace(new RegExp("([a-z]+)=([^;]*);", "g"), (w: string, k: string, v: string) => v + ":" + k + "|"));
console.log("x=é;y=中".replace(/(\w)=(.)/gu, (m: string, k: string, v: string) => v + k));
console.log("x=é;y=😀".replace(/(\w)=(.)/gu, (m: string, k: string, v: string) => v + k));
console.log("x=é;y=😀".replace(/(\w)=(.)/g, (m: string, k: string, v: string) => v + k + m.length));
console.log("😀b".replace(/x*/gu, (m: string) => "<" + m + ">"));
console.log("😀b".replace(/x*/g, (m: string) => "<" + m + ">"));
console.log("éa".replace(/(?:)/g, (m: string) => "|"));
console.log("a-é-中".replace(/-/g, (m: string, off: number, s: string) => "[" + off + "]"));
console.log("a-é-中".replace(new RegExp("-", "g"), (m: string, off: number, s: string) => "[" + off + "]"));

// exec-record shape over non-ASCII subjects
const rec = /([a-z]+)=(\d+)/g;
let total = 0;
for (const line of ["é=1;bb=22;", "中=3;ccc=333;", "😀x=4;"]) {
  rec.lastIndex = 0;
  let m = rec.exec(line);
  while (m !== null) {
    total = total + m.index + m[1].length + parseInt(m[2], 10);
    m = rec.exec(line);
  }
}
console.log(total);

// named groups and `d` indices come from the same spans
pair(/(?<k>[a-z]+)=(?<v>[^;]+)/, new RegExp("(?<k>[a-z]+)=(?<v>[^;]+)"), "é;ab=中文");
pair(/(?<k>[a-z]+)=(?<v>[^;]+)/d, new RegExp("(?<k>[a-z]+)=(?<v>[^;]+)", "d"), "é;ab=中文");
pair(/(a)|(b)/d, new RegExp("(a)|(b)", "d"), "😀b");
