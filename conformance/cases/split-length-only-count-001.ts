// A split product whose only use is its length is never built: the
// lowering counts the tokens. Every count must match the array the
// program would have built, and any other use keeps the array.

// the bench shape: a block binding read only as `.length`
let total: number = 0;
for (let i: number = 0; i < 1000; i = i + 1) {
  let parts: string[] = "3 4 + 2 * 5 +".split(" ");
  total = total + parts.length;
}
console.log(total);

// the direct form, across the spec's edge cases
console.log("a,b,,c".split(",").length, "".split(",").length, "".split("").length);
console.log("abc".split("").length, "aaaa".split("aa").length, "abc".split("abcd").length);
console.log(",abc,".split(",").length, "abc".split("z").length);
// Latin-1 / UTF-16 on either side, surrogate pairs under an empty separator
console.log("é,ü".split(",").length, "日本,語".split(",").length, "abc".split("日").length);
console.log("日日本日".split("日").length, "😀a".split("").length, "a😀b😀".split("😀").length);

// runtime strings through a function body
function lines(s: string): number {
  const ls: string[] = s.split("\n");
  return ls.length;
}
function fields(s: string, sep: string): number {
  return s.split(sep).length;
}
console.log(lines("one\ntwo\nthree"), lines(""), fields("k=v;k2=v2;", ";"), fields("x", ""));

// uses that need the array keep it
function keeps(s: string): void {
  let p: string[] = s.split(" ");
  p.length = 1;
  console.log(p.length, p.join("|"));
  const q: string[] = s.split(" ");
  console.log(q.length, q[0]);
  const r: string[] = s.split(" ");
  const f = (): number => r.length;
  console.log(f());
  const t: string[] = s.split(" ");
  console.log(typeof t.length, t.length + 1);
}
keeps("a b c");

// an inner binding of the same name keeps its own reading
function shadow(s: string): void {
  const a: string[] = s.split(" ");
  {
    const a: number[] = [1, 2, 3];
    console.log(a.length);
  }
  console.log(a.length);
}
shadow("x y");

// a top-level binding, a limit, a RegExp and an undefined separator
const top: string[] = "a b".split(" ");
console.log(top.length, "a b c".split(" ", 2).length, "a1b2c".split(/\d/).length);
console.log("a b".split(undefined).length);
