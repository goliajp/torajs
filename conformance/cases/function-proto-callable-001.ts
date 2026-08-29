// §20.2.3 — %Function.prototype% is a built-in FUNCTION object, not an
// ordinary one. tr minted it as a plain dynobj like every other
// `<Ctor>.prototype`, and one wrong cell tag made three answers wrong
// together: `typeof` said "object", calling it threw, and its
// `toString` fell through to the "[object Function]" badge because
// §20.2.3.5's source-text lane does not recognise a dynobj.
const FP: any = Function.prototype

console.log("typeof:", typeof FP)
console.log("accepts anything, returns undefined:", FP(1, 2, 3), FP())
console.log("toString is a NativeFunction string:", /^function\s*\(\s*\)\s*\{[\s\S]*\[native code\][\s\S]*\}$/.test(FP.toString()))
console.log("name/length:", JSON.stringify(FP.name), FP.length)

// It is a function object without being a function INSTANCE: no
// [[Construct]] and no `prototype` property of its own.
console.log("no prototype prop:", "prototype" in FP, FP.prototype)
console.log("not an instance of itself:", FP instanceof Function, FP instanceof Object)

// §20.1.3.6 still badges it "Function" — that comes from the
// builtinTag walk, not from the cell shape.
console.log("badge:", Object.prototype.toString.call(FP))

// The reflection surface is the one a builtin prototype owes, and it
// no longer depends on which cell shape backs the singleton.
console.log("own names:", JSON.stringify(Object.getOwnPropertyNames(FP).sort()))
console.log("enumerable own:", JSON.stringify(Object.keys(FP)))
const dcall: any = Object.getOwnPropertyDescriptor(FP, "call")
console.log("call descriptor:", JSON.stringify(Object.keys(dcall).sort()), typeof dcall.value,
            dcall.writable, dcall.enumerable, dcall.configurable)
const dlen: any = Object.getOwnPropertyDescriptor(FP, "length")
console.log("length descriptor:", dlen.value, dlen.writable, dlen.enumerable, dlen.configurable)
console.log("hasOwn:", Object.hasOwn(FP, "call"), Object.hasOwn(FP, "constructor"), Object.hasOwn(FP, "toString"))

// §10.2.4's restricted-property accessors survived the shape change.
try { FP.caller } catch (e: any) { console.log("caller throws:", e instanceof TypeError) }

// A prototype is an ordinary mutable object: the rc-immortality flag
// the immortal-cell mint uses would have made it report as frozen.
console.log("mutable:", Object.isFrozen(FP), Object.isExtensible(FP))
;(FP as any).zz = 7
console.log("expando reaches instances:", (function () {} as any).zz)
delete (FP as any).zz

// A delete of an interned method leaves the tombstone every reader
// consults, rather than removing a monkey-patch shadow that is not
// there.
delete FP.bind
console.log("after delete:", typeof (function () {}).bind, "bind" in FP)

console.log("chain:", Object.getPrototypeOf(FP) === Object.prototype,
            Object.getPrototypeOf(function () {}) === FP,
            Object.getPrototypeOf(Function) === FP)
