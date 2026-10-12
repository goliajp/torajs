class C { s: string = "" }
function f(): string {
  const c = new C()
  let y = ""
  for (let i = 0; i < 3; i = i + 1) {
    y = (c.s = "w" + i + "?")
    c.s = "z" + i
  }
  return y
}
console.log(f())
function g(): string {
  const c = new C()
  let out = ""
  for (let i = 0; i < 3; i = i + 1) {
    const y = (c.s = "w" + i + "?")
    c.s = "z" + i
    out = out + y
  }
  return out
}
console.log(g())
function h(): string[] {
  const c = new C()
  const ys: string[] = []
  for (let i = 0; i < 3; i = i + 1) {
    ys.push(c.s = "w" + i + "?")
    c.s = "z" + i
  }
  return ys
}
console.log(h().join(","))
function k(o: any): string {
  let y = ""
  for (let i = 0; i < 3; i = i + 1) {
    y = (o.s = "a" + i + "!")
    o.s = "b" + i
  }
  return y
}
console.log(k({ s: "" }))
function m(): string {
  const xs: string[] = [""]
  let y = ""
  for (let i = 0; i < 3; i = i + 1) {
    y = (xs[0] = "e" + i + ".")
    xs[0] = "f" + i
  }
  const w = (xs[0] = "g" + y)
  xs[0] = "h"
  return y + w
}
console.log(m())
