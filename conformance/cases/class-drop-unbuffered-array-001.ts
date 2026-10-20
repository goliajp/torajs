// Nodes of a tree class also live in an array: the array's release is
// the runtime's, so the nodes keep the cycle buffer and the flag test.
class Expr {
  value: number
  left: Expr | null
  constructor(value: number, left: Expr | null) {
    this.value = value
    this.left = left
  }
}
let sum = 0
for (let r = 0; r < 200; r++) {
  const xs: Expr[] = []
  let prev: Expr | null = null
  for (let i = 0; i < 100; i++) {
    prev = new Expr(r + i, prev)
    xs.push(prev)
  }
  for (let i = 0; i < xs.length; i += 7) {
    const l = xs[i].left
    sum = (sum + xs[i].value + (l === null ? 0 : l.value)) | 0
  }
}
console.log(sum)
