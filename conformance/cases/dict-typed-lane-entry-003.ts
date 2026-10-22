// A getter on the key reads in the update's read: it turns the key into
// a data property, runs an update of the same key itself, and grows the
// dictionary past several resizes before it answers.
const d: Record<string, number> = {}
const ks: string[] = [String.fromCharCode(120)]
const k = ks[0]
let inner = false
Object.defineProperty(d, k, {
  configurable: true,
  enumerable: true,
  get() {
    if (!inner) {
      inner = true
      Object.defineProperty(d, k, { value: 0, writable: true, configurable: true, enumerable: true })
      d[k] = (d[k] ?? 0) + 1
      for (let i = 0; i < 1000; i = i + 1) {
        d["g" + i] = i
      }
    }
    return 5
  },
})
d[k] = (d[k] ?? 0) + 1
console.log(d[k], Object.keys(d).length)
const e: Record<string, number> = {}
Object.defineProperty(e, k, {
  configurable: true,
  enumerable: true,
  get() {
    Object.defineProperty(e, k, { value: 0, writable: false, configurable: false, enumerable: true })
    return 7
  },
})
try {
  ;(() => {
    "use strict"
    e[k] = (e[k] ?? 0) + 1
  })()
  console.log("no throw", e[k])
} catch (err) {
  console.log((err as Error).name, e[k])
}
