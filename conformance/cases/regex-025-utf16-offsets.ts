// match .index / exec lastIndex / replace-cb offset are UTF-16
// code-unit offsets, not transcoded-UTF-8 byte offsets, on
// non-ASCII haystacks (Latin-1 supplement, CJK, astral pairs)
const a: string = "hello 世界 world";
console.log(a.match(/world/));
const b: string = "héllo wörld";
console.log(b.match(/wörld/));
const d: string = "世界 world 世界";
const re = /world/;
console.log(re.exec(d));
const g = /o/g;
const s2: string = "日本öo語o末";
const m1 = g.exec(s2);
console.log(m1, g.lastIndex);
const m2 = g.exec(s2);
console.log(m2, g.lastIndex);
const m3 = g.exec(s2);
console.log(m3, g.lastIndex);
const y = /語/y;
y.lastIndex = 4;
console.log(y.test(s2), y.lastIndex);
y.lastIndex = 5;
console.log(y.test(s2), y.lastIndex);
const r: string = "字x字x".replace(/x/g, (m: string, off: number, input: string): string => {
  return String(off);
});
console.log(r);
const ast: string = "a𝄞b𝄞c";
console.log(ast.match(/c/));
