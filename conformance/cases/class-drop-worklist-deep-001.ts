// Self-referential classes released through long own-class chains,
// walked in one frame instead of one frame per node: a list 50k long,
// a left spine and a right spine 40k deep (the right one fills the
// pending stack and keeps spilling), a right spine of right spines (a
// spill that spills again), a balanced tree, a shared subtree, a
// subclass instance in a child slot, a node kept alive past its tree,
// and WeakRefs into the tree. Every shape is built and dropped twice so a node released
// twice or not at all shows as a crash or a wrong value.
class Node {
  v: number
  next: Node | null = null
  constructor(v: number) { this.v = v }
}
class Tree {
  v: number
  left: Tree | null = null
  right: Tree | null = null
  constructor(v: number) { this.v = v }
}
class Tagged extends Tree {
  label: string
  constructor(v: number, label: string) {
    super(v)
    this.label = label
  }
}
function list(n: number): Node {
  let head = new Node(0)
  for (let i = 1; i < n; i++) {
    const nd = new Node(i)
    nd.next = head
    head = nd
  }
  return head
}
// leftDeep: the spine runs through `left`, leaves hang off `right`;
// rightDeep: the other way round
function spine(n: number, leftDeep: boolean): Tree {
  let root = new Tree(0)
  for (let i = 1; i < n; i++) {
    const t = new Tree(i)
    const leaf = i % 3 === 0 ? null : new Tree(-i)
    if (leftDeep) {
      t.left = root
      t.right = leaf
    } else {
      t.right = root
      t.left = leaf
    }
    root = t
  }
  return root
}
function spineOfSpines(n: number, m: number): Tree {
  let root = new Tree(0)
  for (let i = 1; i < n; i++) {
    const t = new Tree(i)
    t.right = root
    t.left = spine(m, false)
    root = t
  }
  return root
}
function balanced(d: number, v: number): Tree {
  const t = new Tree(v)
  if (d > 0) {
    t.left = balanced(d - 1, v * 2)
    t.right = balanced(d - 1, v * 2 + 1)
  }
  return t
}
function count(t: Tree | null): number {
  let n = 0
  const work: Tree[] = []
  if (t !== null) work.push(t)
  while (work.length > 0) {
    const x = work.pop()!
    n++
    if (x.left !== null) work.push(x.left)
    if (x.right !== null) work.push(x.right)
  }
  return n
}

for (let round = 0; round < 2; round++) {
  let h: Node | null = list(50000)
  console.log("list", h.v, h.next === null ? -1 : h.next.v)
  h = null

  let l: Tree | null = spine(40000, true)
  console.log("left spine", count(l))
  l = null
  let r: Tree | null = spine(40000, false)
  console.log("right spine", count(r))
  r = null
  let ss: Tree | null = spineOfSpines(300, 100)
  console.log("spine of spines", count(ss))
  ss = null

  let b: Tree | null = balanced(16, 1)
  const shared = balanced(4, 7)
  b.left!.left = shared
  b.right!.right = shared
  const kept = b.right!.left!.right!
  b.left!.right = new Tagged(-5, "tagged")
  const refs: WeakRef<Tree>[] = []
  const vals: number[] = []
  for (let t: Tree | null = b; t !== null; t = t.left) {
    refs.push(new WeakRef(t))
    vals.push(t.v)
  }
  console.log("balanced", count(b), count(shared), kept.v)
  b = null
  console.log("after", count(shared), kept.v, count(kept))
  // the first two died with the tree, the rest belong to `shared`
  let ok = true
  for (let i = 0; i < refs.length; i++) {
    const d = refs[i].deref()
    if (i >= 2 ? d === undefined : d !== undefined && d.v !== vals[i]) ok = false
  }
  console.log("weak", ok, refs.length, refs[2].deref() === shared)
}
