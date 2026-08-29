// `recv[k](args)` with a runtime key is a METHOD call on recv
// (§13.3.6.2), and it resolves the name through the same walk the
// dotted spelling does. The walk has a tail — the §10.1.9.2 chain
// step, then %Object.prototype%'s own three — and this lane only ran
// the first half of it, so the three fell past into the value-call
// fallback, which read the inherited function off the chain and
// bare-called it. That is the this-undefined TypeError, and it hit
// every receiver whose own arm does not implement them.
//
// The literal-key spelling never came this way, which is why
// `xs["hasOwnProperty"]("0")` was fine next to a `xs[k]("0")` that
// threw.
const anchor: any = Object
const own: string = "hasOwnProperty"
const enu: string = "propertyIsEnumerable"
const pro: string = "isPrototypeOf"

const shapes: [string, any][] = [
  ["arr  ", [1, 2]],
  ["str  ", "abc"],
  ["num  ", 5],
  ["bool ", true],
  ["map  ", new Map()],
  ["set  ", new Set()],
  ["date ", new Date(0)],
  ["re   ", /a/],
  ["obj  ", { a: 1 }],
  ["cls  ", new (class { b = 1 })()],
]
for (const [n, v] of shapes) {
  console.log("own  " + n, (v as any)[own]("0"), (v as any)["hasOwnProperty"]("0"))
  console.log("enu  " + n, (v as any)[enu]("0"))
  console.log("pro  " + n, (v as any)[pro]([]))
}

// The tail's first half still runs first, so a patch on the same name
// wins over the universal.
;(Object.prototype as any).hasOwnProperty = function () { return "PATCH" }
console.log("patched  :", ([1, 2] as any)[own]("0"))
delete (Object.prototype as any).hasOwnProperty
try { console.log("deleted  :", ([1, 2] as any)[own]("0")) }
catch (e: any) { console.log("deleted  :", e.name) }

// Names the arms DO implement were never affected and must not move.
const j: string = "join"
const s: string = "slice"
const i: string = "indexOf"
console.log("join     :", ([1, 2] as any)[j]("-"))
console.log("slice    :", ("abc" as any)[s](1))
console.log("indexOf  :", ([1, 2] as any)[i](2))
console.log("elem     :", ([10, 20] as any)["1"])
