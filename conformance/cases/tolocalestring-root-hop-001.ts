// §20.1.3.5 — `Object.prototype.toLocaleString` is `Invoke(this,
// "toString")`, not a badge. The two look alike for a receiver whose
// toString IS the badge, which is why answering the badge for every
// arm that floats a miss went unnoticed on Map and Set and showed up
// on a RegExp: `(/a/).toLocaleString()` is "/a/" in every engine.
//
// It is an ORDINARY lookup, so what it resolves to is whatever the
// walk finds when it runs — a patch, a family method, or the root's
// own badge at the end.
const anchor: any = Object

console.log("regexp  :", (/a-b/g as any).toLocaleString())
console.log("u8      :", (new Uint8Array([1, 2]) as any).toLocaleString())
console.log("map     :", (new Map() as any).toLocaleString())
console.log("set     :", (new Set() as any).toLocaleString())
console.log("weakmap :", (new WeakMap() as any).toLocaleString())
console.log("promise :", (Promise.resolve(1) as any).toLocaleString())
console.log("symbol  :", (Symbol("s") as any).toLocaleString())
console.log("bigint  :", (10n as any).toLocaleString())
console.log("numwrap :", ((Object as any)(5) as any).toLocaleString())
console.log("strwrap :", ((Object as any)("s") as any).toLocaleString())
console.log("arr     :", ([1, 2] as any).toLocaleString())
console.log("num     :", (5 as any).toLocaleString())
console.log("str     :", ("ab" as any).toLocaleString())
console.log("bool    :", (true as any).toLocaleString())

// The hop is a lookup: patch the toString the walk would reach and the
// inherited toLocaleString follows it.
;(RegExp.prototype as any).toString = function () { return "RE" }
console.log("re patch:", (/a-b/g as any).toLocaleString())
delete (RegExp.prototype as any).toString
console.log("re gone :", (/a-b/g as any).toLocaleString())

// A family that owns its own toLocaleString never takes the hop.
console.log("num own :", (1234567 as any).toLocaleString())
