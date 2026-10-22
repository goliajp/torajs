// Holes from a grow and a delete, then a non-integral read on
// number[], string[] and boolean[] receivers: no named key, so each
// answers undefined.
const n: number[] = [1, 2]
const s: string[] = ["x", "y"]
const t: boolean[] = [true]
const far = parseInt(process.argv[2] ?? "4", 10)
s[far] = "z"
t[far] = false
delete n[0]
const k = parseFloat("0.5")
console.log(s[k], t[k], n[k], s.length, t.length, s[3], t[2], n[0])
