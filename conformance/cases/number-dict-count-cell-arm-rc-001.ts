// A number dictionary counted with `d[k] = (d[k] ?? 0) + 1` while an
// `any` alias stores other values under some keys. The value the read
// answers is released where the int, double and `+` kernel arms meet;
// only the kernel arm holds a cell there, so the release runs on that
// arm alone. Each step frees the previous string, and the counted
// value moves into the entry: a release that came too early would
// show in the strings, one too many at teardown.
const counts: Record<string, number> = {}
const alias: any = counts
alias["s"] = "str"
alias["o"] = { toString() { return "obj" } }
alias["arr"] = [1, 2]
alias["big"] = 2147483647
alias["half"] = 1.5
alias["nan"] = NaN
const keys = ["s", "o", "arr", "big", "half", "nan", "new", "s"]
for (let i = 0; i < 4000; i = i + 1) {
  const k = keys[i % 8]
  counts[k] = (counts[k] ?? 0) + 1
}
const s = String(alias["s"])
console.log(s.length, s.slice(0, 8), s.slice(-4))
console.log(String(alias["o"]).slice(0, 6), String(alias["o"]).length)
console.log(String(alias["arr"]).slice(0, 6), String(alias["arr"]).length)
console.log(counts["big"], counts["half"], counts["nan"], counts["new"])

// the same count through a function, so the read and the write sit in
// a loop of their own with the arms joining inside it
function tally(d: Record<string, number>, ks: string[], rounds: number): number {
  let total = 0
  for (let r = 0; r < rounds; r = r + 1) {
    for (let j = 0; j < ks.length; j = j + 1) {
      const k = ks[j]
      d[k] = (d[k] ?? 0) + 1
      total = total + 1
    }
  }
  return total
}
const d2: Record<string, number> = {}
const d2any: any = d2
d2any["x"] = "x"
d2any["y"] = 2147483646
console.log(tally(d2, ["x", "y", "z", "x"], 500), String(d2any["x"]).length, d2["y"], d2["z"])
console.log(Object.keys(d2).join(","), typeof d2["x"], typeof d2["y"])
