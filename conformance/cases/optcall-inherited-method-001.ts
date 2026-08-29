// `o.m?.()` decides whether the arguments evaluate by probing
// whether the callee slot resolves — ES §13.3.9's GetV. For a plain
// object that probe read the OWN slot only, on the reasoning that a
// dynobj has nothing above it. That stopped being true when
// %Object.prototype% went on the chain: `o.hasOwnProperty?.("a")`
// answered undefined in front of a dispatcher that resolves it, and
// so did every method reached through `Object.create(p)`.
//
// The three Object.prototype probes were the registered symptom;
// user prototypes are the same miss one link further out.

const o: any = { a: 1 }
console.log(o.hasOwnProperty?.("a"), o.hasOwnProperty?.("zz"))
console.log(o.propertyIsEnumerable?.("a"))
console.log(o.isPrototypeOf?.({}))
console.log(typeof o.valueOf?.(), typeof o.toString?.())

// A user prototype, reached the ordinary way.
const proto: any = { greet() { return "hi" }, n: 7 }
const child: any = Object.create(proto)
console.log(child.greet?.())
console.log(child.missing?.())

// Two links up.
const gchild: any = Object.create(child)
console.log(gchild.greet?.())

// An own entry storing undefined still shadows what is above it.
const shadow: any = Object.create(proto)
shadow.greet = undefined
console.log(shadow.greet?.())

// A null-prototype object has none of them, and the arguments must
// not evaluate.
let ran = 0
const bare: any = Object.create(null)
console.log(bare.hasOwnProperty?.((ran++, "a")), ran)

// The short-circuit still holds where the name genuinely is absent:
// the argument expression never runs.
let ran2 = 0
console.log(o.nope?.((ran2++, 1)), ran2)

// A non-callable resolved value is still the catchable TypeError.
const nc: any = { m: 5 }
try { nc.m?.() ; console.log("no throw") } catch (e: any) { console.log(e.name) }

// Not covered here, and recorded rather than papered over: an
// ACCESSOR inherited from a user prototype. `Object.create({ get
// f() { … } }).f` answers undefined on a plain read and throws on a
// plain call, so the optional call has nothing to be consistent with
// yet; it now fails the same way the plain call does instead of
// silently answering undefined.
