// TS §2.7.2 — `unknown` is the top type: any value, but member access
// / arithmetic without a narrowing guard fails at type-check time. The
// subset collapses `unknown` to `Type::Any` (runtime behaviour is
// identical); the no-access-without-narrow constraint is independent
// substrate work (mirror of the `object` non-primitive constraint).
// Pre-fix tr rejected `: unknown` at the type-annotation resolver
// ("unknown type `unknown` for parameter…"). Fix accepts it as an
// alias of `any`.

// Parameter ann
function isStr(x: unknown): boolean {
  return typeof x === "string"
}
console.log(isStr("hello"))
console.log(isStr(42))

// Variable ann
const raw: unknown = "json-like"
console.log(typeof raw)

// Return ann
function passthrough(x: unknown): unknown {
  return x
}
console.log(passthrough(7))
console.log(passthrough("ok"))

// `unknown` field on a class
class Box {
  v: unknown
  constructor(v: unknown) { this.v = v }
}
const b = new Box("hello")
console.log(typeof b.v)

// typeof predicate against `unknown` — runtime tag check; member
// access on a narrowed branch is independent substrate (Any.member
// dispatch gap, separate L3b).
function kind(v: unknown): string {
  if (typeof v === "string") return "str"
  if (typeof v === "number") return "num"
  if (typeof v === "boolean") return "bool"
  return "other"
}
console.log(kind("hi"))
console.log(kind(42))
console.log(kind(true))
console.log(kind(null))
