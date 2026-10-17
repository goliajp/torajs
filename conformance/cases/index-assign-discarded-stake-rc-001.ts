// An index assignment whose value nothing reads takes no stake for a
// consumer: a statement, a `for` step and the last operand of a
// discarded comma store the value and leave its count to the slot.
// A kept value (`c = (o[k] = v)`, a chained assignment, an operand)
// still holds a reference of its own after the slot lets go of it.
const d: Record<string, number> = {}
const da: any = d
const o: any = {}
const arr: any = [0, 0, 0]
const xs: any[] = ["a", "b"]
let tag = ""
for (let i = 0; i < 2000; i = i + 1) {
  const k = "k" + (i % 3)
  o[k] = "v" + i
  arr[i % 3] = [i, "e" + i]
  xs[i % 2] = "x" + i
  da[k] = "n" + i
  ;(tag = "t" + i), (o["c"] = "c" + i)
}
console.log(o.k0, o.k1, o.k2, o.c, tag)
console.log(String(arr[0]), String(arr[1]), String(arr[2]))
console.log(xs.join("|"), d["k0"], d["k1"])

const steps: any = {}
let n = 0
for (let i = 0; i < 1000; i = i + 1, steps["s" + (i % 2)] = "step" + i) {
  n = n + 1
}
console.log(n, steps.s0, steps.s1)

// kept values: each keeps the string after the slot is overwritten
const kept: any[] = []
for (let i = 0; i < 4; i = i + 1) {
  const c = (o["w"] = "w" + i)
  kept.push(c)
  let e: any
  let f: any
  e = f = o["w"] = ["kept", i]
  o["w"] = 0
  kept.push(String(e), String(f))
}
console.log(kept.join(","), o["w"])
const sum = (arr[0] = "p") + (arr[1] = "q")
arr[0] = 1
arr[1] = 2
console.log(sum, arr[0], arr[1])
