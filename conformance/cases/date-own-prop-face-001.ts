// A Date instance is an ordinary object whose [[DateValue]] is
// internal state (§21.4.4), so a plain assign is an own property and
// never disturbs the time value.
const d: any = new Date(86400000)
d.note = "birthday"
console.log(d.note, d.getTime(), d.toISOString())
console.log(Object.keys(d), Object.getOwnPropertyNames(d))
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(d, "note")))
console.log(JSON.stringify(d))

const sym = Symbol("s")
d[sym] = 3
console.log(d[sym], Object.getOwnPropertySymbols(d).length)

Object.defineProperty(d, "hidden", { value: 5, enumerable: false })
console.log(d.hidden, Object.keys(d), Object.getOwnPropertyNames(d))

delete d.note
console.log(d.note, "note" in d, d.getTime())
