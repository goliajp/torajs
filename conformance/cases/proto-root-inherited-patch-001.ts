// The patch bitmap the dispatcher's pre-arm consult reads is keyed
// per (prototype, mid), so a write to %Object.prototype% was
// invisible to every other family's gate — and the hop to the root
// lives inside the slot lookup, which the gate had already declined
// to reach. The arms that answer an INHERITED name natively therefore
// kept answering it.

;(Object.prototype as any).valueOf = function () {
  return "patched-v"
}
console.log(([1] as any).valueOf())
console.log((new Map() as any).valueOf())
console.log((new Set() as any).valueOf())
console.log((/x/ as any).valueOf())
console.log((function () {} as any).valueOf())

// Ownership is the whole of the condition: a family that owns the
// name reaches its own, not the root's (§22.1.3.35 String, §21.1.3
// Number, §20.3.3 Boolean, §21.4.4 Date, §20.4.3 Symbol, §21.2.3
// BigInt all own valueOf).
console.log(("ab" as any).valueOf(), (5 as any).valueOf(), (true as any).valueOf())
console.log(typeof (new Date() as any).valueOf(), (1n as any).valueOf())

// An own entry still comes first — the consult stands down on it.
const withOwn: any = [1]
withOwn.valueOf = () => "own"
console.log(withOwn.valueOf())

// Same story under toString, where Array owns one (§23.1.3.36) and
// Map / Set do not, so only the latter reach the root.
;(Object.prototype as any).toString = function () {
  return "patched-t"
}
console.log(([1, 2] as any).toString(), ({} as any).toString())
console.log((new Map() as any).toString(), (new Set() as any).toString())

// And under toLocaleString (§20.1.3.5), which Map does not own.
;(Object.prototype as any).toLocaleString = function () {
  return "patched-l"
}
console.log((new Map() as any).toLocaleString(), ({} as any).toLocaleString())
