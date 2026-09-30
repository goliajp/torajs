// A constructor storing an object into a field of `this` releases
// what the field held: null from the factory, an initializer's
// object, or what an earlier statement of the same constructor put
// there. A frozen `this` still refuses the store (§10.1.9.2), and the
// value that was about to be stored stays usable by its other owners.
class Node {
  v: number
  next: Node | null
  constructor(v: number, next: Node | null) {
    this.v = v
    this.next = next
  }
}
function sum(n: Node | null): number {
  let s = 0
  while (n !== null) {
    s = s + n.v
    n = n.next
  }
  return s
}
let list: Node | null = null
for (let i = 1; i <= 5; i = i + 1) {
  list = new Node(i, list)
}
console.log("list:", sum(list))

class Holder {
  inner: Node | null = new Node(7, null)
  constructor(n: Node | null, replace: boolean) {
    if (replace) {
      this.inner = n
    }
  }
}
console.log("kept:", sum(new Holder(null, false).inner), "replaced:", sum(new Holder(list, true).inner))

class Over {
  inner: Node | null = new Node(9, null)
  constructor(n: Node | null) {
    this.inner = n
  }
}
console.log("over:", sum(new Over(list).inner), sum(new Over(null).inner))

class Twice {
  a: Node | null
  constructor(x: Node, y: Node) {
    this.a = x
    this.a = y
  }
}
const t = new Twice(new Node(1, null), new Node(2, null))
console.log("twice:", sum(t.a))

class Last {
  a: Node | null = null
  constructor(xs: Node[]) {
    for (let i = 0; i < xs.length; i = i + 1) {
      this.a = xs[i]
    }
  }
}
const xs = [new Node(3, null), new Node(4, null), new Node(5, null)]
console.log("last:", sum(new Last(xs).a), sum(xs[0]) + sum(xs[1]) + sum(xs[2]))

class Locked {
  child: Node | null = null
  constructor(n: Node) {
    Object.freeze(this)
    this.child = n
  }
}
const shared = new Node(11, new Node(12, null))
try {
  new Locked(shared)
  console.log("locked no throw")
} catch (e) {
  console.log("locked threw:", e instanceof TypeError, sum(shared))
}

class LockedLate {
  a: Node | null = null
  b: Node | null = null
  constructor(n: Node) {
    this.a = n
    Object.freeze(this)
    this.b = n
  }
}
try {
  new LockedLate(shared)
  console.log("late no throw")
} catch (e) {
  console.log("late threw:", e instanceof TypeError, sum(shared))
}

class Base {
  p: Node | null
  constructor(n: Node | null) {
    this.p = n
  }
}
class Derived extends Base {
  q: Node | null
  constructor(n: Node, m: Node) {
    super(n)
    this.q = m
    this.p = m
  }
}
const d = new Derived(new Node(20, null), new Node(30, null))
console.log("derived:", sum(d.p), sum(d.q))

let churn = 0
for (let i = 0; i < 20000; i = i + 1) {
  const o = new Over(new Node(i, null))
  churn = (churn + sum(o.inner)) | 0
}
console.log("churn:", churn)
