// A family prototype giving a name up does not end the walk — it hands
// the call to %Object.prototype%, whose own method then answers. The
// sibling 002 pins the READ side of the same delete (the name stays a
// function because the root still has one); this pins WHICH function
// runs, which is the half 002 explicitly left unasserted.
//
// The three names are the only ones a family can both own and share
// with the root, and each root answer is a different shape: §20.1.3.6
// classifies a badge, §20.1.3.7 is ToObject (so a primitive receiver
// stops being a primitive), and §20.1.3.5 is a plain
// Invoke(this, "toString") with no locale grouping in it.
const anchor: any = Object

// §20.1.3.6 — the badge, once the family's own toString is gone.
console.log("arr  pre :", ([1, 2] as any).toString())
delete (Array.prototype as any).toString
console.log("arr  post:", ([1, 2] as any).toString())
delete (Number.prototype as any).toString
console.log("num  post:", (5 as any).toString())
delete (String.prototype as any).toString
console.log("str  post:", ("ab" as any).toString())
delete (Boolean.prototype as any).toString
console.log("bool post:", (true as any).toString())
delete (RegExp.prototype as any).toString
console.log("re   post:", (/a/ as any).toString())
delete (Symbol.prototype as any).toString
console.log("sym  post:", (Symbol("s") as any).toString())
delete (BigInt.prototype as any).toString
console.log("big  post:", (10n as any).toString())

// A family that never owned the name keeps its own arm: `Map.prototype`
// has no toString, so nothing was given up and the badge it already
// answered is still the badge the root would give.
console.log("map      :", (new Map() as any).toString())
console.log("set      :", (new Set() as any).toString())

// The receiver's own face and a restore both outrank the redirect —
// §10.1.8.1 resolves the own property first, and putting a function
// back on the prototype revives it with no clear call.
const own: any = [3, 4]
own.toString = function () { return "own" }
console.log("own      :", own.toString())
;(Array.prototype as any).toString = function () { return "restored" }
console.log("restored :", ([3, 4] as any).toString())
delete (Array.prototype as any).toString
console.log("regone   :", ([3, 4] as any).toString())

// §20.1.3.7 — ToObject(this), so the answer is an object, not the
// primitive the family's own valueOf would have unwrapped to.
console.log("num vpre :", typeof (5 as any).valueOf())
delete (Number.prototype as any).valueOf
console.log("num vpost:", typeof (5 as any).valueOf())
delete (String.prototype as any).valueOf
console.log("str vpost:", typeof ("ab" as any).valueOf())
delete (Boolean.prototype as any).valueOf
console.log("bool vpst:", typeof (true as any).valueOf())

// §20.1.3.5 — Invoke(this, "toString"), which is an ordinary lookup:
// with Number's own toString already deleted above it resolves to the
// root's, so the grouped digits the family's toLocaleString produced
// are gone.
console.log("num lpre :", (1234567 as any).toLocaleString())
delete (Number.prototype as any).toLocaleString
console.log("num lpost:", (1234567 as any).toLocaleString())

// Once the ROOT gives it up too there is no supplier left anywhere and
// the call is the one 001 pins: not a function, on every receiver.
delete (Object.prototype as any).toString
try { console.log("gone arr :", ([1, 2] as any).toString()) }
catch (e: any) { console.log("gone arr :", e.name) }
try { console.log("gone map :", (new Map() as any).toString()) }
catch (e: any) { console.log("gone map :", e.name) }
