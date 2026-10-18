// Chains of class instances that hold each other, released when the
// last reference to the head goes. The release fns of classes that
// reach one another call each other once per level, and a worklist
// whose pending stack fills releases the overflow by calling itself:
// the native depth must stay bounded whatever the depth of the data.
// Two classes alternating through fields, two classes alternating
// through an array field, a class reaching itself through
// an object type and an array, and a self-referential class whose
// worklist overflows on every level (33 references per node).
class A {
  v: number
  next: B | null = null
  constructor(v: number) { this.v = v }
}
class B {
  s: string
  next: A | null = null
  constructor(s: string) { this.s = s }
}
class P {
  q: Q[] = []
}
class Q {
  n: number
  p: P | null = null
  constructor(n: number) { this.n = n }
}
type T = { a: Box[] }
class Box {
  v: number
  t: T | null = null
  constructor(v: number) { this.v = v }
}
class W {
  v: number
  kids: W[] = []
  constructor(v: number) { this.v = v }
}
const names = ["b0", "b1", "b2"]

function ab(n: number): A {
  let h = new A(0)
  for (let i = 1; i < n; i++) {
    const b = new B(names[i % 3])
    b.next = h
    const a = new A(i)
    a.next = b
    h = a
  }
  return h
}
function pq(n: number): P {
  let h = new P()
  for (let i = 0; i < n; i++) {
    const q = new Q(i)
    q.p = h
    const p = new P()
    p.q.push(q)
    h = p
  }
  return h
}
function boxes(n: number): Box {
  let h = new Box(0)
  for (let i = 1; i < n; i++) {
    const b = new Box(i)
    b.t = { a: [h] }
    h = b
  }
  return h
}
function caterpillar(n: number): W {
  const leaf = new W(-1)
  let h = new W(0)
  for (let i = 1; i < n; i++) {
    const p = new W(i)
    for (let j = 0; j < 32; j++) p.kids.push(leaf)
    p.kids.push(h)
    h = p
  }
  return h
}
function depthAB(a: A | null): number {
  let d = 0
  while (a !== null) {
    d++
    const b: B | null = a.next
    a = b === null ? null : b.next
  }
  return d
}

for (let round = 0; round < 2; round++) {
  let a: A | null = ab(300000)
  console.log("ab", a.v, a.next!.s, depthAB(a))
  a = null
  let p: P | null = pq(300000)
  console.log("pq", p.q.length, p.q[0].n)
  p = null
  let b: Box | null = boxes(300000)
  console.log("box", b.v, b.t!.a[0].v)
  b = null
  let w: W | null = caterpillar(100000)
  console.log("caterpillar", w.v, w.kids.length, w.kids[32].v)
  w = null
}
console.log("done")
