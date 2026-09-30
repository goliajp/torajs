// Every read the view lane answers without copying: length,
// charCodeAt, equality, console.log, a bare return, and a group that
// did not participate arriving as undefined.

console.log("aa b ccc".replace(/\w+/g, (m: string) => m.length + ""))
console.log("abc".replace(/./g, (m: string) => m.charCodeAt(0) + ","))
console.log("abc".replace(/./g, (m: string) => m.charCodeAt() + ";"))
console.log("a-b-a".replace(/[ab]/g, (m: string) => (m === "a" ? "A" : m)))
console.log("a-b-a".replace(/[ab]/g, (m: string) => (m !== "a" ? "B" : m)))
console.log("xyz".replace(/y/, (m: string) => m))
console.log(
  "p1 q2".replace(/(\w)(\d)/g, (m: string, a: string, b: string) => {
    console.log(m, a, b)
    return b + a
  }),
)
// `(a)|(b)`: one of the two groups never participates
console.log("ab".replace(/(a)|(b)/g, (m: string, x: string, y: string) => "<" + x + "|" + y + ">"))
console.log("ab".replace(/(a)|(b)/g, (m: string, x: string, y: string) => (x === undefined ? "?" : x) + (y === undefined ? "?" : y)))
// fewer parameters than groups
console.log("2026-05-21".replace(/(\d+)-(\d+)-(\d+)/, (m: string, y: string) => y))
// a closure that counts across matches
let n = 0
console.log(
  "a b c".replace(/\w/g, (m: string) => {
    n = n + 1
    return m + n
  }),
)
console.log(n)
