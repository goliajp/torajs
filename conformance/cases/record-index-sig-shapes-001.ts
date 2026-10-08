// Index signatures in every declared position: a literal-union key, a
// parameter, a return type, interfaces with and without named members,
// a type alias, and a number index signature.

const r: Record<'a' | 'b', number> = { a: 1, b: 2 }
console.log(r.a + r.b, Object.keys(r).join(","))
r.a = 10
console.log(JSON.stringify(r))
function total(d: { [k: string]: number }): number {
  let s = 0
  for (const k in d) s += d[k]
  return s
}
function make(n: number): Record<string, number> {
  const out: Record<string, number> = {}
  for (let i = 0; i < n; i++) out["k" + i] = i * 2
  return out
}
const m = make(4)
console.log(total(m), Object.values(m).join(" "))
console.log(Object.entries(m).map(([k, v]) => k + "=" + v).join(";"))
interface Scores { [name: string]: number }
const sc: Scores = { amy: 3 }
sc.bob = 4
console.log(JSON.stringify(sc), "amy" in sc, "carl" in sc)
interface Named { title: string; [k: string]: string }
const nm: Named = { title: "t", extra: "e" }
nm.more = "m"
console.log(nm.title, nm["extra"], Object.keys(nm).join(","))
type Bag = { [k: string]: string[] }
const bag: Bag = {}
bag["x"] = ["1"]; bag["x"].push("2")
console.log(JSON.stringify(bag))
const arrLike: { [i: number]: string } = {}
arrLike[0] = "zero"; arrLike[5] = "five"
console.log(arrLike[0], arrLike[5], arrLike[1], Object.keys(arrLike).join(","))
