class A {
  v = 1
}
class B {
  v = 2
}
let y = 0
const a = new ((y = 3), A)()
console.log(a.v, y)
const b = new (0, B)
console.log(b.v)
let log: string[] = []
const pick = (s: string) => {
  log.push(s)
  return s === "a" ? A : B
}
const c = new (pick("x"), pick("a"))()
console.log(c.v, log.join(","))
for (let i = new (y > 0 ? A : B)().v; i < 3; i++) {
  console.log("i", i)
}
