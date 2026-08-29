// §10.1.8.1 step 4 / §7.3.12 — both the builtin-prototype VALUE read
// and the `in` face walk the WHOLE chain, one link at a time. Two
// links ("family, else root") was enough only while every builtin
// prototype hung straight off %Object.prototype%; §23.1.5.2 gives an
// array iterator three.
;(Object.prototype as any).foo = 5

// A TYPED array receiver reaches the root the way an `any`-bound one
// already did.
const a: number[] = [1, 2]
console.log((a as any).foo, ([] as any).foo)

// The prototype object itself inherits from the root too.
console.log(typeof (Array as any).prototype.hasOwnProperty)
console.log(typeof (Map as any).prototype.hasOwnProperty)

// Family beats root; an own entry storing undefined shadows the root.
;(Object.prototype as any).zz = "root"
;(Array as any).prototype.zz = "family"
console.log((a as any).zz)
;(Array as any).prototype.zz = undefined
console.log((a as any).zz)

// The `in` face over the three-link iterator chain.
const it: any = [1].values()
console.log("next" in it, "hasOwnProperty" in it, "nope" in it)
;(Iterator as any).prototype.qq = 9
console.log("qq" in it, it.qq)
console.log("next" in (new Map() as any).entries(), "next" in (new Set() as any).values())
const h: any = [1].values().map((x: number) => x)
console.log("next" in h, "toArray" in h)

// Nothing about the spec faces moved.
console.log([3, 1, 2].map((x: number) => x + 1), [1, 2].join("-"))
console.log((a as any).toString === (Array as any).prototype.toString)
delete (Object.prototype as any).foo
delete (Object.prototype as any).zz
