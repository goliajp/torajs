// A number-typed operand that holds an object at run time: its valueOf
// runs between the read and the write of the update and grows the
// dictionary past several resizes.
const d: Record<string, number> = {}
const ks: string[] = [String.fromCharCode(120), String.fromCharCode(121)]
const kx = ks[0]
const ky = ks[1]
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
  d[kx] = (d[kx] ?? 0) + n
}
d[kx] = 0
for (let r = 0; r < 3; r = r + 1) {
  ;(add as any)(bomb)
}
console.log(d[kx], Object.keys(d).length)
let n: number = bomb as any
d[ky] = 0
for (let r = 0; r < 3; r = r + 1) {
  d[ky] = (d[ky] ?? 0) + n
}
console.log(d[ky], Object.keys(d).length)
