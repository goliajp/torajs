// Subjects the walk transcodes — Latin-1 beyond ASCII, UTF-16, an
// astral pair — and the empty match's advance through each.

console.log("café-naïve".replace(/[a-z]+/g, (m: string) => m.length + ""))
console.log("日本語 text 中文".replace(/\w+/g, (m: string) => "<" + m + ">"))
console.log("a𝄞b".replace(/𝄞/u, (m: string) => "[" + m.length + "]"))
console.log("ab".replace(/x*/g, (m: string) => "<" + m + ">"))
console.log("𝄞b".replace(/x*/gu, (m: string) => "<" + m + ">"))
console.log("𝄞b".replace(/x*/g, (m: string) => "<" + m + ">"))
console.log("éa".replace(/(?:)/g, (m: string) => "|"))
console.log("".replace(/x*/g, (m: string) => "<" + m + ">"))
// a view of a non-ASCII subject appended onto an ASCII result
console.log("x=é;y=中".replace(/(\w)=(.)/gu, (m: string, k: string, v: string) => v + k))
// the result of a replace is itself replaced
const once = "a-b-c".replace(/-/g, (m: string) => "é")
console.log(once.replace(/é/g, (m: string) => "+" + m.length))
