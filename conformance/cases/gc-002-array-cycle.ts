// V3-09 — Array slot participates in cycle collection.
// `parent.kids[0] === child` and `child.parent === parent` form
// a cycle that routes through an Array<Node> slot rather than a
// direct class-field reference. Without V3-09's TAG_ARR walker
// branches in mark_gray / scan / scan_black / collect_white,
// the cycle would survive the trial-deletion pass — the Array
// would block descent into the inner class instance.

class Node {
  v: number;
  kids: Node[];
  parent: Node | null;
  constructor(v: number) { this.v = v; this.kids = []; this.parent = null; }
}

let root = new Node(1)
let child = new Node(2)
root.kids.push(child)
child.parent = root

console.log(root.v)
console.log(root.kids.length)
console.log(root.kids[0].v)
console.log(child.parent === null)

Bun.gc(true)
console.log('after gc')
