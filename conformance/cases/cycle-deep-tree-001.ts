// Deep binary trees under the cycle collector: a left spine 200k
// nodes deep with a leaf hung off every seventh node, grown root-first
// the way a moving head pointer grows a list (every old root becomes a
// cycle candidate whose walk reaches the whole spine below it), and a
// complete tree of depth 19 built bottom-up. Both are walked while
// alive under a forced collect, then released.
class Tree {
  v: number
  left: Tree | null = null
  right: Tree | null = null
  constructor(v: number) { this.v = v }
}
function spine(n: number): Tree {
  let root = new Tree(0)
  for (let i = 1; i < n; i++) {
    const t = new Tree(i)
    t.left = root
    t.right = i % 7 === 0 ? new Tree(-i) : null
    root = t
  }
  return root
}
function spineSum(t: Tree | null): number {
  let s = 0
  while (t !== null) {
    s += t.v % 1000
    if (t.right !== null) s += 1
    t = t.left
  }
  return s
}
function complete(depth: number, v: number): Tree {
  const t = new Tree(v)
  if (depth > 0) {
    t.left = complete(depth - 1, v * 2)
    t.right = complete(depth - 1, v * 2 + 1)
  }
  return t
}
function count(t: Tree | null): number {
  if (t === null) return 0
  return 1 + count(t.left) + count(t.right)
}
let s: Tree | null = spine(200000)
Bun.gc(true)
console.log(spineSum(s))
s = null
Bun.gc(true)
let c: Tree | null = complete(19, 1)
Bun.gc(true)
console.log(count(c))
c = null
Bun.gc(true)
console.log(s === null && c === null)
