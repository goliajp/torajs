// The one-call dynamic-key read on receivers that are not ordinary
// objects keeps each receiver's own answer: arrays, functions,
// strings, a Proxy (string and symbol keys reach the get trap), a
// throwing getter, and a null receiver (TypeError).
const ks: string[] = ["length", "0", "x", "name", "missing"]
const arr: any = [10, 20]
arr[ks[2]] = "expando"
console.log(arr[ks[0]], arr[ks[1]], arr[ks[2]], arr[ks[4]], typeof arr["push"])
function named(a: number, b: number): number {
  return a + b
}
const fn: any = named
fn[ks[2]] = 5
console.log(fn[ks[3]], fn[ks[0]], fn[ks[2]], fn[ks[4]])
const str: any = "hello"
console.log(str[ks[0]], str[ks[1]], str[ks[4]], typeof str["slice"])
const seen: string[] = []
const sym = Symbol("p")
const target: any = { x: 1 }
target[sym] = "sym value"
const px: any = new Proxy(target, {
  get(t: any, k: any, r: any) {
    seen.push(typeof k === "symbol" ? "sym" : k)
    return t[k]
  },
})
console.log(px[ks[2]], px[ks[4]], px[sym], seen.join(","))
const thrower: any = {}
Object.defineProperty(thrower, "x", {
  get() {
    throw new RangeError("from getter")
  },
})
try {
  console.log(thrower[ks[2]])
} catch (e) {
  console.log(e instanceof RangeError, (e as any).message)
}
const nul: any = ks.length > 100 ? {} : null
try {
  console.log(nul[ks[2]])
} catch (e) {
  console.log(e instanceof TypeError)
}
const und: any = ks.length > 100 ? {} : undefined
try {
  console.log(und[sym])
} catch (e) {
  console.log(e instanceof TypeError)
}
// a class instance read through any with a dynamic key
class P {
  a = 1
  get b(): number {
    return this.a + 1
  }
}
const inst: any = new P()
const kb: string = ks.length > 0 ? "b" : "a"
console.log(inst[kb], inst["a"])
