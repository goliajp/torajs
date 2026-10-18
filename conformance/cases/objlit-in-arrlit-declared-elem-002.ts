// An array literal lowered against an array of object types lays its
// object literal elements out at the declared element type whatever
// the array type is spelled as (a type alias, `readonly`,
// `ReadonlyArray<T>`) and whatever lane builds the literal (a spread
// item, a literal spread source, a hole). Each `any` member of an
// element is then cast into its slot: an object with the fields
// crosses as a copy, and an instance of a class without them throws a
// TypeError where bun stores it and reads `undefined` from it; those
// lines differ from bun and are checked against the .expected. A
// module-level `let`, an assignment to it, and elements with an
// optional field present in only some of them are laid out the same
// way.
class X {
  n = 5
}
type S = { s: string }
type L = { t: S }[]
type RL = readonly { t: S }[]

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError)
  }
}
function alias(v: any): string {
  const a: L = [{ t: v }]
  return a[0].t.s
}
function readonlyAlias(v: any): string {
  const a: RL = [{ t: v }]
  return a[0].t.s
}
function readonlyArray(v: any): string {
  const a: ReadonlyArray<{ t: S }> = [{ t: v }]
  return a[0].t.s
}
function spreadItem(v: any): string {
  const base: { t: S }[] = [{ t: { s: "b" } }]
  const a: { t: S }[] = [...base, { t: v }]
  return a[0].t.s + a[1].t.s
}
function spreadSource(v: any): string {
  const a: { t: S }[] = [...[{ t: v }], { t: { s: "c" } }]
  return a[0].t.s + a[1].t.s
}
function hole(v: any): string {
  const a: { t: S }[] = [{ t: v }, , { t: v }]
  return a[0].t.s + a.length
}
let held: { t: S }[] = [{ t: { s: "init" } }]
function assigned(v: any): string {
  held = [{ t: v }, { t: { s: "b" } }]
  return held[0].t.s + held[1].t.s
}
function optionalField(): string {
  const a: { x: number; y?: string }[] = [{ x: 1 }, { x: 2, y: "a" }]
  const b: { x: number; y?: string }[] = [{ x: 3, y: "b" }, { x: 4 }]
  return [a[0].x, a[0].y, a[1].x, a[1].y, b[0].x, b[0].y, b[1].x, b[1].y].join(" ")
}

const good: any = { s: "ok" }
const inst: any = new X()
console.log(held[0].t.s, held.length)
attempt("alias", () => console.log(alias(good)))
attempt("readonly alias", () => console.log(readonlyAlias(good)))
attempt("ReadonlyArray", () => console.log(readonlyArray(good)))
attempt("spread item", () => console.log(spreadItem(good)))
attempt("spread source", () => console.log(spreadSource(good)))
attempt("hole", () => console.log(hole(good)))
attempt("assign", () => console.log(assigned(good)))
attempt("optional field", () => console.log(optionalField()))
attempt("alias instance", () => console.log(typeof alias(inst)))
attempt("readonly alias instance", () => console.log(typeof readonlyAlias(inst)))
attempt("ReadonlyArray instance", () => console.log(typeof readonlyArray(inst)))
attempt("spread item instance", () => console.log(typeof spreadItem(inst)))
attempt("spread source instance", () => console.log(typeof spreadSource(inst)))
attempt("hole instance", () => console.log(typeof hole(inst)))
attempt("assign instance", () => console.log(typeof assigned(inst)))
