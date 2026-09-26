// copies that read a string[] through [[Get]] (Array.from, spread,
// toReversed, toSorted, toSpliced, with) turn each hole into an
// undefined element: callbacks see undefined, search finds it, sort
// puts it last; slice / concat keep the hole itself
function kinds(xs: string[]): string {
  const out: string[] = []
  xs.forEach(x => out.push(typeof x))
  return out.join(",")
}
function report(label: string, s: string[]): void {
  console.log(label, kinds(Array.from(s)), kinds([...s]), kinds(s.toReversed()), kinds(s.toSorted()))
  console.log(label, kinds(s.toSpliced(0, 1)), kinds(s.with(1, "w")), kinds(s.slice()), kinds(s.concat(["c"])))
  const a = Array.from(s)
  const b = [...s]
  console.log(label, a.length, a.indexOf(undefined as any), b.includes(undefined as any), b.lastIndexOf(undefined as any))
  console.log(label, a.join("-"), JSON.stringify(b), s.toSorted().join("-"), s.toReversed().join("-"))
  console.log(label, 0 in s.slice(), 0 in a, a.map(x => x === undefined ? "U" : x).join(""))
}

const d: string[] = ["p", "q", "r"]
delete d[0]
report("delete", d)

const g: string[] = ["m", "n"]
g.length = 4
report("length", g)

const w: string[] = ["k"]
w[3] = "z"
report("oob", w)

const parts = "aa bb cc".split(" ")
delete parts[1]
report("split", parts)

const n: number[] = [1, 2, 3]
delete n[1]
const nk: string[] = []
Array.from(n).forEach(x => nk.push(typeof x));
[...n].forEach(x => nk.push(typeof x))
n.toReversed().forEach(x => nk.push(typeof x))
console.log(nk.join(","), n.toSorted().join("-"))
