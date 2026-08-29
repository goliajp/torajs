// §27.1.2 gives %Iterator.prototype% two ACCESSOR properties where
// every other builtin prototype carries data: `constructor`
// (§27.1.2.1) and `[Symbol.toStringTag]` (§27.1.2.2). The reason is
// that this prototype sits under every iterator in the language, so
// a writable data property would let `it.constructor = x` — an
// ordinary assignment on an ordinary object — rewrite the shared
// root. The setter is SetterThatIgnoresPrototypeProperties: writing
// through an instance defines the property on the instance, writing
// on the home object throws.
//
// Before this, `constructor` was the synthesized data property every
// builtin prototype gets, `[Symbol.toStringTag]` was absent, and
// `Iterator.prototype.constructor = 7` silently replaced the root's
// constructor for the rest of the process.

const d1: any = Object.getOwnPropertyDescriptor(Iterator.prototype, "constructor")
console.log(d1 ? Object.keys(d1).sort().join(",") : "absent")
const d2: any = Object.getOwnPropertyDescriptor(Iterator.prototype, Symbol.toStringTag)
console.log(d2 ? Object.keys(d2).sort().join(",") : "absent")
console.log(d1.enumerable, d1.configurable, d2.enumerable, d2.configurable)

// The getters answer %Iterator% and "Iterator" — the badge falls out
// of the second one.
console.log((Iterator.prototype as any).constructor === Iterator)
console.log((Iterator.prototype as any)[Symbol.toStringTag])
console.log(Object.prototype.toString.call(Iterator.prototype))

// An iterator inherits both, and a nearer prototype still shadows:
// %ArrayIteratorPrototype% owns its own tag.
const it: any = [1].values()
console.log(it.constructor === Iterator, it[Symbol.toStringTag])
console.log(Object.getOwnPropertyDescriptor(it, "constructor"))

// Writing through an instance lands on the instance. The root does
// not move.
it.constructor = 5
console.log(Object.getOwnPropertyDescriptor(it, "constructor")?.value)
console.log((Iterator.prototype as any).constructor === Iterator)

const o: any = Object.create(Iterator.prototype)
o[Symbol.toStringTag] = "Zed"
console.log(Object.getOwnPropertyDescriptor(o, Symbol.toStringTag)?.value)
console.log(Object.prototype.toString.call(o))
console.log((Iterator.prototype as any)[Symbol.toStringTag])

// A second write through the same instance keeps hitting the own
// entry it just made, not the inherited setter.
o[Symbol.toStringTag] = "Zed2"
console.log(o[Symbol.toStringTag], (Iterator.prototype as any)[Symbol.toStringTag])

// Writing ON the home object throws — the note in the spec says this
// emulates assignment to a non-writable data property.
try { (Iterator.prototype as any).constructor = 7; console.log("no throw") } catch (e: any) { console.log(e.name) }
try { (Iterator.prototype as any)[Symbol.toStringTag] = 7; console.log("no throw") } catch (e: any) { console.log(e.name) }
console.log((Iterator.prototype as any).constructor === Iterator, (Iterator.prototype as any)[Symbol.toStringTag])

// A primitive receiver has no property table (step 1).
try { d1.set.call(1, 5); console.log("no throw") } catch (e: any) { console.log("primitive", e.name) }

// The pair is reachable as values, and the faces are functions.
console.log(typeof d1.get, typeof d1.set, typeof d2.get, typeof d2.set)
console.log(d1.get.call(undefined) === Iterator, d2.get.call(undefined))
