// An array holding its class in place, stored in a field of a
// container (which marks the array's element kind in its header),
// with a cycle collection forced while containers are shared, after
// some are released, and at the end: the collection must leave the
// in-place elements alone.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

class Node {
  toks: Tok[]
  tag: number
  constructor(toks: Tok[], tag: number) {
    this.toks = toks
    this.tag = tag
  }
}

function make(n: number): Tok[] {
  const ts: Tok[] = []
  for (let i = 0; i < n; i = i + 1) ts.push(new Tok(i, i * 10))
  return ts
}

function weigh(n: Node): number {
  let s = n.tag
  for (let i = 0; i < n.toks.length; i = i + 1) s = s + n.toks[i].value
  return s
}

let kept = 0
const live = new Node(make(6), 1)
for (let round = 0; round < 4; round = round + 1) {
  const a = new Node(make(5 + round), round)
  const b = a
  b.toks.push(new Tok(99, 1))
  kept = kept + weigh(a) + weigh(b)
  Bun.gc(true)
}
Bun.gc(true)
live.toks.push(new Tok(7, 7))
Bun.gc(true)
console.log(kept, weigh(live), live.toks.length, live.toks[6].kind)
