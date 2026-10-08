// Members separated by line breaks beside an index signature, a readonly
// signature, string and number signatures together, alias and enum keys,
// an array into a number signature, a generic groupBy and a recursive
// dictionary type.

interface Env {
  name: string
  [key: string]: string
}
const env: Env = { name: "dev", region: "eu" }
env.zone = "a"
console.log(env.name, Object.keys(env).join(","))
interface Lookup {
  readonly [id: string]: number
}
const lk: Lookup = { a: 1 }
console.log(lk["a"], lk.b)
type Both = { [k: string]: number; [i: number]: number }
const both: Both = { 1: 10, x: 20 }
both[2] = 30
console.log(JSON.stringify(both))
type Key = "left" | "right"
const sides: Record<Key, number> = { left: 1, right: 2 }
console.log(sides.left + sides["right"])
enum Color { Red = "red", Blue = "blue" }
const paint: Record<Color, number> = { [Color.Red]: 1, [Color.Blue]: 2 }
console.log(JSON.stringify(paint))
const arrIdx: { [i: number]: string } = ["p", "q"]
console.log(arrIdx[0], arrIdx[1])
function groupBy<T>(xs: T[], key: (x: T) => string): Record<string, T[]> {
  const out: Record<string, T[]> = {}
  for (const x of xs) {
    const k = key(x)
    if (!(k in out)) out[k] = []
    out[k].push(x)
  }
  return out
}
const g = groupBy([1, 2, 3, 4, 5], (n) => (n % 2 === 0 ? "even" : "odd"))
console.log(JSON.stringify(g))
type Tree = { [name: string]: Tree }
const t: Tree = { a: { b: {} } }
t.c = {}
console.log(JSON.stringify(t), Object.keys(t["a"]).length)
const ks: Record<string | number, boolean> = {}
ks[1] = true; ks["two"] = false
console.log(JSON.stringify(ks))
