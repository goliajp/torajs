// Dictionaries nested in dictionaries, arrays and class fields; an
// optional parameter, reassignment, spread, a cast of JSON.parse, a union
// and a function value type, and a closure holding one.

const nested: Record<string, Record<string, number>> = {}
nested["x"] = {}
nested["x"]["y"] = 1
nested.z = { w: 2 }
console.log(JSON.stringify(nested))
const list: Record<string, number>[] = [{ a: 1 }, {}]
list[1]["b"] = 2
console.log(JSON.stringify(list), list.length)
class Registry {
  items: Record<string, string> = {}
  add(k: string, v: string): void { this.items[k] = v }
  count(): number { return Object.keys(this.items).length }
}
const reg = new Registry()
reg.add("p", "q"); reg.add("r", "s")
console.log(reg.count(), JSON.stringify(reg.items))
function opt(d?: Record<string, number>): number { return d ? Object.keys(d).length : -1 }
console.log(opt(), opt({ a: 1, b: 2 }))
let cur: Record<string, boolean> = { on: true }
cur = { off: false, on: true }
console.log(JSON.stringify(cur))
const copy: Record<string, number> = { ...{ a: 1 }, b: 2 }
console.log(JSON.stringify(copy))
const parsed = JSON.parse('{"k":1,"j":2}') as Record<string, number>
console.log(Object.keys(parsed).join(","), parsed["k"] + parsed["j"])
const mixed: Record<string, string | number> = {}
mixed.a = 1; mixed.b = "two"
console.log(JSON.stringify(mixed), typeof mixed.a, typeof mixed["b"])
const fnDict: Record<string, (n: number) => number> = { dbl: (n) => n * 2 }
fnDict.inc = (n) => n + 1
console.log(fnDict["dbl"](4), fnDict.inc(4))
const mkCounter = () => {
  const seen: { [k: string]: number } = {}
  return (k: string): number => { seen[k] = (seen[k] || 0) + 1; return seen[k] }
}
const ctr = mkCounter()
ctr("a"); ctr("b")
console.log(ctr("a"))
const d2: Record<string, number> = { a: 1 }
console.log(d2.hasOwnProperty("a"), "toString" in d2, Object.entries(d2).length)
for (const [k, v] of Object.entries(d2)) console.log(k, v + 1)
const e1: Record<number, string> = { 1: "one" }
e1[2] = "two"
console.log(JSON.stringify(e1), e1[1])
const inv: Record<string, string> = {}
const src: Record<string, string> = { a: "x", b: "y" }
for (const k of Object.keys(src)) inv[src[k]] = k
console.log(JSON.stringify(inv))
console.log(Object.values(src).map((s) => s.toUpperCase()).join(""))
