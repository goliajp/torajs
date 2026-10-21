// A regex literal is compiled at build time and its RegExp is built
// from that image: named groups, backreferences (a forward one
// included), capture counts and the `d` indices must read the same
// as a pattern parsed at run time.

const date = /(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})/;
const m = date.exec("on 2026-10-22 at noon");
if (m !== null) {
  console.log(m[0], m.index, m.length);
  console.log(m.groups!.year, m.groups!.month, m.groups!.day);
}

const twice = /(\w+) \1/;
console.log(twice.test("hello hello"), twice.test("hello world"));
const tw = twice.exec("say bye bye now");
if (tw !== null) console.log(tw[0], tw[1], tw.index);

// a backreference to a group that has not matched yet matches empty
const fwd = /\2(a)(b)/;
const f = fwd.exec("xab");
if (f !== null) console.log(f[0], f[1], f[2], f.index);

const named = /\k<tag>(?<tag>x)y/;
const n = named.exec("--xy");
if (n !== null) console.log(n[0], n.groups!.tag, n.index);

const kv = /(?<k>[a-z]+)=(?<v>\d+)/g;
let hit: RegExpExecArray | null;
while ((hit = kv.exec("a=1, bb=22, ccc=333")) !== null) {
  console.log(hit.groups!.k, hit.groups!.v, kv.lastIndex);
}

const ind = /(?<w>b+)(c)?/d;
const di = ind.exec("aabbbd");
if (di !== null) {
  console.log(JSON.stringify(di.indices));
  console.log(JSON.stringify(di.indices!.groups));
}

const alt = /(?:(?<a>x)|(?<b>y))z/;
const al = alt.exec("yz");
if (al !== null) console.log(al.groups!.a, al.groups!.b);

console.log("a1b22c333".replace(/(?<d>\d+)/g, "[$<d>]"));
console.log("2026-10-22".replace(date, "$<day>/$<month>/$<year>"));
