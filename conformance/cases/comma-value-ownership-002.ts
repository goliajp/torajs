const n = parseInt("3", 10)
function t(xs: string[]): number { return xs.length }
function r(k: number, a: string): string { return (k, a) }
const a = "ke" + n
let y = ""
let c = 0
for (let i = 0; i < n; i = i + 1) {
  const w = (i, a)
  y = (i, a)
  const z = (i, "z" + i)
  c = c + w.length + t([(i, a)]) + z.length + r(i, a).length
}
console.log(c, y)

function churn(): number {
  const j: string[] = []
  for (let i = 0; i < 200; i = i + 1) j.push("j" + i)
  return j.length
}
function u(s: string): number { return s.length }
function arrElem(n: number): string {
  const a = "ae" + n
  let c = 0
  for (let i = 0; i < n; i = i + 1) c = c + t([(i, a)])
  churn()
  return a + c
}
function arrElemPair(n: number): string {
  const a = "af" + n
  const out: string[][] = []
  for (let i = 0; i < 3; i = i + 1) out.push([(i, a), "q"])
  churn()
  return a + out[2][0] + out[2][1]
}
function objField(n: number): string {
  const a = "of" + n
  const os: { k: string }[] = []
  for (let i = 0; i < 3; i = i + 1) os.push({ k: (i, a) })
  churn()
  return a + os[1].k
}
function pushArg(n: number): string {
  const a = "pa" + n
  const xs: string[] = []
  for (let i = 0; i < 3; i = i + 1) xs.push((i, a))
  churn()
  return a + xs.join("")
}
function callArg(n: number): string {
  const a = "ca" + n
  let c = 0
  for (let i = 0; i < n; i = i + 1) c = c + u((i, a))
  churn()
  return a + c
}
function ret(n: number): string {
  const a = "rt" + n
  let s = ""
  for (let i = 0; i < 3; i = i + 1) s = s + ((): string => (i, a))()
  churn()
  return a + s
}
function letInit(n: number): string {
  const a = "li" + n
  let c = 0
  for (let i = 0; i < n; i = i + 1) {
    let w = (i, a)
    c = c + w.length
  }
  churn()
  return a + c
}
function assignValue(n: number): string {
  const a = "as" + n
  let y = ""
  for (let i = 0; i < n; i = i + 1) y = (i, a)
  churn()
  return a + y
}
function concatOperand(n: number): string {
  const a = "co" + n
  let s = ""
  for (let i = 0; i < 3; i = i + 1) s = s + (i, a) + "-"
  churn()
  return a + s
}
function arrNum(n: number): string {
  const a = [n, 1]
  let c = 0
  for (let i = 0; i < n; i = i + 1) c = c + [(i, a)].length
  churn()
  return a.join("") + c
}
function arrArr(n: number): number {
  const a = [n, 1]
  const out: number[][] = []
  for (let i = 0; i < 3; i = i + 1) out.push((i, a))
  churn()
  return a.length + out[2].length + out[2][0]
}
function mapSet(n: number): string {
  const a = "ms" + n
  const m = new Map<string, string>()
  for (let i = 0; i < 3; i = i + 1) m.set("k" + i, (i, a))
  churn()
  return a + m.get("k1")
}
function memberAssign(n: number): string {
  const a = "ma" + n
  const o = { k: "" }
  for (let i = 0; i < 3; i = i + 1) o.k = (i, a)
  churn()
  return a + o.k
}
function indexAssign(n: number): string {
  const a = "ia" + n
  const xs = ["", ""]
  for (let i = 0; i < 3; i = i + 1) xs[1] = (i, a)
  churn()
  return a + xs[1]
}
function thrown(n: number): string {
  const a = "th" + n
  let r = ""
  for (let i = 0; i < 3; i = i + 1) {
    try {
      throw (i, a)
    } catch (e) {
      r = r + e
    }
  }
  churn()
  return a + r
}
function ternary(n: number): string {
  const a = "te" + n
  const xs: string[] = []
  for (let i = 0; i < 3; i = i + 1) xs.push(i > 0 ? (i, a) : a)
  churn()
  return a + xs.join("")
}
function nested(n: number): string {
  const a = "ne" + n
  const xs: string[] = []
  for (let i = 0; i < 3; i = i + 1) xs.push((i, (i, a)))
  churn()
  return a + xs.join("")
}
function anyLane(v: any): string {
  const b = "ag" + v
  const x = [(0, b), "q"]
  return x.join("-")
}
const k = n + 2
console.log(arrElem(k))
console.log(arrElemPair(k))
console.log(objField(k))
console.log(pushArg(k))
console.log(callArg(k))
console.log(ret(k))
console.log(letInit(k))
console.log(assignValue(k))
console.log(concatOperand(k))
console.log(arrNum(k))
console.log(arrArr(k))
console.log(mapSet(k))
console.log(memberAssign(k))
console.log(indexAssign(k))
console.log(thrown(k))
console.log(ternary(k))
console.log(nested(k))
console.log(anyLane(n))
