// §22.2.4.1 keeps `lastIndex` in the RegExp cell rather than in the
// lazy own-property bag, which is why the define ladder held that
// one name out of the bag arm — and then had nowhere else to put
// it. `Object.defineProperty(re, "lastIndex", {value: 3})` reached
// the "no expando define storage" arm and was dropped in silence:
// the old value stayed, and the call answered as though it had
// worked. The property is `{writable: true, enumerable: false,
// configurable: false}`, so §10.1.6.3 also lets it go read-only
// exactly once, and nothing could express that either.

const re: any = /a/g
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(re, "lastIndex")))
console.log(Object.getOwnPropertyNames(re).join(","))

re.lastIndex = 5
console.log(re.lastIndex)
Object.defineProperty(re, "lastIndex", { value: 3 })
console.log(re.lastIndex)
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(re, "lastIndex")))

// A non-numeric value stores verbatim — ToLength happens at the
// §22.2.7.2 exec entry, not here.
Object.defineProperty(re, "lastIndex", { value: "7" })
console.log(re.lastIndex, typeof re.lastIndex)
Object.defineProperty(re, "lastIndex", { value: 0 })

// The attributes that cannot move.
for (const d of [{ configurable: true }, { enumerable: true }]) {
  try { Object.defineProperty(re, "lastIndex", d as any); console.log("no throw") }
  catch (e: any) { console.log(e.name) }
}
console.log(Reflect.defineProperty(re, "lastIndex", { configurable: true }))

// The one attribute change §10.1.6.3 allows on a non-configurable
// data property: writable true -> false, once.
Object.defineProperty(re, "lastIndex", { writable: false })
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(re, "lastIndex")))

// Frozen: the value it already holds is still a legal redefine
// (SameValue), a different one is not, and assignment refuses.
Object.defineProperty(re, "lastIndex", { value: 0 })
console.log("same-value ok", re.lastIndex)
try { Object.defineProperty(re, "lastIndex", { value: 9 }); console.log("no throw") }
catch (e: any) { console.log(e.name) }
try { Object.defineProperty(re, "lastIndex", { writable: true }); console.log("no throw") }
catch (e: any) { console.log(e.name) }
try { re.lastIndex = 7; console.log("assign ok", re.lastIndex) }
catch (e: any) { console.log(e.name, re.lastIndex) }
console.log(Reflect.defineProperty(re, "lastIndex", { value: 9 }))
console.log(Object.getOwnPropertyNames(re).join(","))

// A fresh RegExp is unaffected — the bit is per cell.
const re2: any = /b/g
re2.lastIndex = 4
console.log(re2.lastIndex, JSON.stringify(Object.getOwnPropertyDescriptor(re2, "lastIndex")))

// Every other name on a RegExp still lands in the bag.
Object.defineProperty(re2, "zz", { value: 1, enumerable: true, configurable: true })
console.log(re2.zz, Object.getOwnPropertyNames(re2).join(","))
