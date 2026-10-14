// Object.fromEntries over a Set walks the iteration protocol, and an
// object entry reads "0" / "1" with [[Get]] whatever its shape
const s = new Set<any>([["a", 1], { 0: "b", 1: 2 }])
console.log(JSON.stringify(Object.fromEntries(s)))
try { Object.fromEntries(new Set<any>([["ok", 1], 7])) } catch (e: any) { console.log(e.constructor.name) }
let got = 0
const lazy = new Set<any>([{ get "0"() { got++; return "g" }, get "1"() { got++; return 3 } }])
console.log(JSON.stringify(Object.fromEntries(lazy)), got)
const inherited: any = Object.create({ 0: "p", 1: 4 })
console.log(JSON.stringify(Object.fromEntries([inherited])))
const own: any = {}
Object.defineProperty(own, "0", { get() { return "d" } })
Object.defineProperty(own, "1", { value: 5 })
console.log(JSON.stringify(Object.fromEntries([own])))
// closing a plain-object iterator reads `return` with [[Get]]
let closes = 0
function walk(it: any): void {
  const src: any = { [Symbol.iterator]() { return it } }
  try {
    for (const x of src) { console.log("item", x); break }
    console.log("closed", closes)
  } catch (e: any) { console.log(e.constructor.name, e instanceof RangeError ? e.message : "") }
}
const step = () => ({ done: false, value: 1 })
walk({ next: step, return() { closes++; return {} } })
walk({ next: step, get return() { return () => { closes++; return {} } } })
walk({ next: step, get return() { return undefined } })
walk({ next: step, return: null })
walk({ next: step, return: 3 })
walk({ next: step, get return() { throw new RangeError("getter") } })
walk({ next: step, return() { return 1 } })
