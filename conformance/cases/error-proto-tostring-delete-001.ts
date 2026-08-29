// The struct arm's version of the same walk: §20.5.3.4 belongs to
// `Error.prototype`, so deleting it there has to hand the call on to
// %Object.prototype% rather than leave the arm rendering
// `name: message` from a fixed offset.
//
// The chain probe already answered "no override" for two different
// states — the entry is absent, and the entry IS the builtin — and
// conflating them is what kept the render alive. They are different
// answers: absent means the walk continues.
//
// The badge that then answers is `[object Error]`, not
// `[object Object]`: §20.1.3.6 step 12 names a receiver with an
// [[ErrorData]] slot, and the struct path used to skip the classifier
// that knows it.
const anchor: any = Object
class Sub extends Error { }

const e0: any = new Error("m")
console.log("pre  any  :", e0.toString())
const t0: Error = new Error("m")
console.log("pre  typed:", t0.toString())
console.log("pre  badge:", (Object.prototype.toString as any).call(e0))

// A prototype override wins over both — it is what the chain resolves.
;(Error.prototype as any).toString = function () { return "OVR" }
console.log("ovr  any  :", (new Error("m") as any).toString())
const t1: Error = new Error("m")
console.log("ovr  typed:", t1.toString())

// Deleting it leaves nothing on the chain below %Object.prototype%.
delete (Error.prototype as any).toString
console.log("del  own  :", (Error.prototype as any).hasOwnProperty("toString"))
console.log("del  typeof:", typeof (Error.prototype as any).toString)
const e2: any = new Error("m")
console.log("del  any  :", e2.toString())
const t2: Error = new Error("m")
console.log("del  typed:", t2.toString())
console.log("del  String:", String(e2))
console.log("del  tmpl :", `${e2}`)
console.log("del  sub  :", (new Sub("s") as any).toString())
console.log("del  type :", (new TypeError("t") as any).toString())

// A subclass that carries its own entry still wins — the chain is
// walked from the instance up, so the delete above never mattered to
// it.
;(Sub.prototype as any).toString = function () { return "SUBP" }
console.log("sub  own  :", (new Sub("s") as any).toString())

// Neighbours the badge classifier must keep answering as before.
class Plain { }
console.log("plain     :", (new Plain() as any).toString())
console.log("obj       :", ({ a: 1 } as any).toString())
console.log("map       :", (new Map() as any).toString())

// Once %Object.prototype% gives it up too there is no supplier left.
delete (Object.prototype as any).toString
try { console.log("gone any  :", (new Error("m") as any).toString()) }
catch (x: any) { console.log("gone any  :", x.name) }
try { const t3: Error = new Error("m"); console.log("gone typed:", t3.toString()) }
catch (x: any) { console.log("gone typed:", x.name) }
