function f(n: number): string {
  const a = "ke" + n
  for (let i = 0; i < 3; i = i + 1) {
    const w = (i, a)
    if (w.length > 100) console.log("x")
  }
  let y = ""
  for (let i = 0; i < 3; i = i + 1) {
    y = (i, a)
  }
  const junk: string[] = []
  for (let i = 0; i < 50; i = i + 1) junk.push("j" + i)
  return a + y + junk.length
}
console.log(f(7))
function g(n: number): number[] {
  const xs = [n, n + 1]
  for (let i = 0; i < 3; i = i + 1) {
    const w = (i, xs)
    w.push(i)
  }
  return xs
}
console.log(g(1).join(","))
function r(n: number): string {
  const a = "re" + n
  return (n, a)
}
function q(n: number): string {
  const a = "qe" + n
  const b = (r(n), a)
  return b + (0, "x" + n)
}
const keep: string[] = []
for (let i = 0; i < 3; i = i + 1) keep.push(r(i) + q(i))
const junk: string[] = []
for (let i = 0; i < 50; i = i + 1) junk.push("j" + i)
console.log(keep.join(","))
function t2(xs: string[]): number { return xs.length }
const base = "b" + keep.length
let c = 0
for (let i = 0; i < 3; i = i + 1) c = c + t2([(i, base)])
console.log(c, base)
