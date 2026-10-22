// A number-typed operand that holds an object at run time: its valueOf
// runs between the read and the write of the update and grows the
// dictionary past several resizes.
const d: Record<string, number> = {}
let grown = 0
const bomb = {
  valueOf() {
    for (let i = 0; i < 1000; i = i + 1) {
      d["k" + grown + "_" + i] = i
    }
    grown = grown + 1
    return 1
  },
}
function add(n: number) {
  d["x"] = (d["x"] ?? 0) + n
}
d["x"] = 0
for (let r = 0; r < 3; r = r + 1) {
  ;(add as any)(bomb)
}
console.log(d["x"], Object.keys(d).length)
let n: number = bomb as any
d["y"] = 0
for (let r = 0; r < 3; r = r + 1) {
  d["y"] = (d["y"] ?? 0) + n
}
console.log(d["y"], Object.keys(d).length)
