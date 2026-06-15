// S127-2 — `v == null` for `v: Any` per spec §7.2.13. Loose-eq nullish
// must be true for both null and undefined values, false for any
// concrete primitive / heap. Pre-fix typecheck rejected
// `strict equality requires same primitive type, got Any and Null`
// because js_loose_eq_supported didn't cover Any.
const xs: any[] = [1, "two", null, undefined, 0, false, ""]
// .some / .every / .find idiomatic nullish guard
console.log("any nullish?", xs.some(v => v == null))
console.log("any non-nullish?", xs.some(v => v != null))
console.log("all non-nullish?", xs.every(v => v != null))
console.log("first nullish idx =", xs.findIndex(v => v == null))
// Direct check on each slot
const tags: string[] = []
for (let i = 0; i < xs.length; i++) {
  tags.push(i + ":" + (xs[i] == null ? "NULLISH" : "VAL"))
}
console.log("per-slot:", tags.join("|"))
// Reverse order also (Null == Any)
console.log("rev nullish first =", xs.findIndex(v => null == v))
// LooseNeq path
const nonNullish = xs.filter(v => v != null)
console.log("nonNullish.length =", nonNullish.length)
