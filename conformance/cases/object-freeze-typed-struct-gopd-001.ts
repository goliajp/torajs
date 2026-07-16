// RFC 20260716 刀 25 — Object.freeze on typed struct (class instance):
// gOPD reflects FLAG_FROZEN / FLAG_SEALED. Pre-fix `struct_reflect`
// hard-coded `{writable: true, configurable: true}` regardless of the
// cell's integrity level. Post-fix ES §7.3.14 SetIntegrityLevel:
// - frozen  ⇒ writable=false, configurable=false
// - sealed  ⇒ writable preserved, configurable=false
// The header 3 flags (isFrozen / isSealed / isExtensible) and the
// write-side throw were already spec-correct pre-fix.

class Point {
  x: number = 1
  y: number = 2
}

function report(label: string, p: Point) {
  const dx = Object.getOwnPropertyDescriptor(p, "x")
  console.log(
    label,
    "isFrozen=" + Object.isFrozen(p),
    "isSealed=" + Object.isSealed(p),
    "isExtensible=" + Object.isExtensible(p),
  )
  console.log(
    "  x",
    "value=" + (dx && dx.value),
    "W=" + (dx && dx.writable),
    "E=" + (dx && dx.enumerable),
    "C=" + (dx && dx.configurable),
  )
}

// Baseline (no integrity level) — every field descriptor is fully
// writable / enumerable / configurable.
const p0 = new Point()
report("baseline", p0)

// Sealed only — configurable false, writable preserved.
const ps = new Point()
Object.seal(ps)
report("sealed", ps)

// Frozen — both writable and configurable false.
const pf = new Point()
Object.freeze(pf)
report("frozen", pf)

// Sanity — writable still true on a fresh instance made post-freeze
// of a different one.
const pOther = new Point()
report("other post-freeze", pOther)

// The write-side already threw pre-fix — regression sentinel.
try {
  pf.x = 99
  console.log("write to frozen did NOT throw, pf.x=", pf.x)
} catch (e) {
  console.log("write to frozen threw:", (e as any).message ?? String(e))
}
