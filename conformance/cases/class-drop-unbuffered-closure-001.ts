// A closure captures a node of a tree class and is called after the
// node is replaced: the node lives in a capture box the runtime holds,
// so nothing about the tree's release is settled and it keeps the
// cycle buffer and the flag test.
class Expr {
  value: number
  left: Expr | null
  constructor(value: number, left: Expr | null) {
    this.value = value
    this.left = left
  }
}
let sum = 0
for (let i = 0; i < 5000; i++) {
  let e = new Expr(i, new Expr(i * 2, null))
  const read = () => e.value + (e.left === null ? 0 : e.left.value)
  sum = (sum + read()) | 0
  e = new Expr(-i, null)
  sum = (sum + read()) | 0
}
console.log(sum)
