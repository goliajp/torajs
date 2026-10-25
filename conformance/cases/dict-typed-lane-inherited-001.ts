// Run-time keys on a `Record<string, number>` that name members the
// object inherits from Object.prototype (§10.1.8.1 OrdinaryGet walks
// the chain on a miss): `constructor`, `toString`, `hasOwnProperty`
// answer functions and `__proto__` runs the inherited getter. Every
// answer here is only added, compared or `typeof`-ed, and the program
// names no constructor or namespace; the keys are put together at run
// time, so no literal spells one.
const d: Record<string, number> = { a: 1, b: 2 }
const heads: string[] = ["constr", "toStr", "hasOwnPro", "__pr", "", "z"]
const tails: string[] = ["uctor", "ing", "perty", "oto__", "a", "z"]
const ks: string[] = []
for (let i = 0; i < heads.length; i = i + 1) {
  ks.push(heads[i] + tails[i])
}
let line = ""
for (const k of ks) {
  const v = d[k]
  line = line + k + ":" + typeof v + ":" + (v === undefined) + " "
}
console.log(line)
console.log("" + d[ks[1]])
console.log(d[ks[3]] + "|" + (d[ks[5]] ?? 0) + "|" + (d[ks[4]] + 1))
// a number written under `__proto__` reaches the inherited setter,
// which ignores a non-object (B.2.2.1.2 step 4)
d[ks[3]] = 5
console.log(typeof d[ks[3]], Object.keys(d).length)
let sum = 0
for (const k of Object.keys(d)) {
  sum = sum + d[k]
}
console.log(sum, d[ks[0]] === d[ks[0]], d[ks[1]] == d[ks[2]])
