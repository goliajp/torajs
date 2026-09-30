// The «matched, captures…, position, string» shape: position is a
// UTF-16 unit offset and the input is the whole subject.

console.log("a1b22".replace(/\d+/g, (m: string, off: number, s: string) => "[" + m + "@" + off + "/" + s.length + "]"))
console.log(
  "k=v;x=y".replace(/(\w)=(\w)/g, (m: string, a: string, b: string, off: number, s: string) => a + b + off + (s === "k=v;x=y" ? "!" : "?")),
)
// offsets count UTF-16 units past a two-unit code point
console.log("𝄞a𝄞b".replace(/[ab]/g, (m: string, off: number, s: string) => m + off))
console.log("é1ü2".replace(/\d/g, (m: string, off: number, s: string) => "(" + off + ")"))
