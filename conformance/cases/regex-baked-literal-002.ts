// Flags, `source` and `lastIndex` of build-time compiled literals.

const res: RegExp[] = [
  /a/g,
  /a/i,
  /^a$/m,
  /a.b/s,
  /a/u,
  /[\p{L}--[a-z]]/v,
  /a/y,
  /a/d,
  /a/gimsuyd,
  /\//,
  /[/]x/,
  /(?:)/,
  /\u{1F600}/u,
];
for (const re of res) {
  console.log(re.source, re.flags, re.global, re.ignoreCase, re.multiline);
  console.log(re.dotAll, re.unicode, re.unicodeSets, re.sticky, re.hasIndices);
  console.log(String(re));
}

const g = /o/g;
console.log(g.lastIndex, g.test("foo"), g.lastIndex, g.test("foo"), g.lastIndex);
console.log(g.test("foo"), g.lastIndex);
g.lastIndex = 2;
console.log(g.exec("foo")?.index, g.lastIndex);

const y = /o/y;
console.log(y.test("foo"), y.lastIndex);
y.lastIndex = 1;
console.log(y.test("foo"), y.lastIndex, y.test("foo"), y.lastIndex, y.test("foo"), y.lastIndex);

console.log(/HELLO/i.test("say hello"), /^b/m.test("a\nb"), /^b/.test("a\nb"));
console.log(/a.c/.test("a\nc"), /a.c/s.test("a\nc"));
console.log(/^.$/.test("😀"), /^.$/u.test("😀"), "😀".length);
console.log("x😀y".match(/./gu)!.length, "x😀y".match(/./g)!.length);

function fresh(): RegExp {
  return /z/g;
}
const a = fresh();
a.lastIndex = 3;
const b = fresh();
console.log(a === b, a.lastIndex, b.lastIndex);
