// The call channel's half of 001. The read channel learned to ask
// whether the supplying prototype still has the name; the arms that
// ANSWER a call did not, and they answer natively — the badge
// classifier, the identity valueOf, the toLocaleString hop and the
// three own-property probes are all %Object.prototype%'s own surface
// given without a walk, which is exactly why no tombstone could
// reach them. `delete Object.prototype.hasOwnProperty` left
// `({ a: 1 } as any).hasOwnProperty("a")` answering true where both
// bun and V8 have nothing left to resolve and throw.
//
// valueOf and toLocaleString show it twice over: after toString
// alone is deleted, `({}).valueOf()` still answers the object, and
// printing that object is what throws — §7.1.1.1 has no toString and
// no valueOf-that-returns-a-primitive left to try.
//
// Receivers include the substrate's own prototype singletons on
// purpose: last rotation a change to a lookup serving all receivers
// went out with 28 green probes that were all plain objects and
// arrays, and the gate caught seven cases whose receiver was a
// builtin prototype.
const anchor: any = Object
function t(label: string, f: () => any): void {
  try { console.log(label, ":", String(f())) } catch (e: any) { console.log(label, ": throws", e.constructor.name) }
}
function sweep(tag: string): void {
  t(tag + " obj toString  ", () => ({} as any).toString())
  t(tag + " obj valueOf   ", () => ({} as any).valueOf())
  t(tag + " obj toLocale  ", () => ({} as any).toLocaleString())
  t(tag + " obj hasOwn    ", () => ({ a: 1 } as any).hasOwnProperty("a"))
  t(tag + " obj propEnum  ", () => ({ a: 1 } as any).propertyIsEnumerable("a"))
  t(tag + " obj isProtoOf ", () => ({} as any).isPrototypeOf({}))
  t(tag + " Oproto valueOf", () => (Object.prototype as any).valueOf())
  t(tag + " Mproto toLoc  ", () => (Map.prototype as any).toLocaleString())
  t(tag + " err toString  ", () => (new Error("boom") as any).toString())
  t(tag + " map toString  ", () => (new Map() as any).toString())
  t(tag + " num valueOf   ", () => (5 as any).valueOf())
  t(tag + " str valueOf   ", () => ("s" as any).valueOf())
}
sweep("pre ")
const O: any = Object.prototype
delete O.toString
sweep("d-ts")
delete O.valueOf
sweep("d-vo")
delete O.toLocaleString
delete O.hasOwnProperty
delete O.propertyIsEnumerable
delete O.isPrototypeOf
sweep("d-al")
