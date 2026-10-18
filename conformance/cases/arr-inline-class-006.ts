// A push onto an array of in-place instances read out of a field, whose
// argument overwrites that field or rebinds the object holding it: the
// element goes to the array the place held before the arguments ran,
// and that array lives until the push is done.

class Tok {
  kind: number
  value: number
  constructor(k: number, v: number) {
    this.kind = k
    this.value = v
  }
}
class Node {
  toks: Tok[]
  constructor(t: Tok[]) {
    this.toks = t
  }
}
function fresh(): Tok[] {
  const ts: Tok[] = []
  return ts
}

const n = new Node(fresh())
function swap(m: Node): number {
  m.toks = fresh()
  return 3
}
n.toks.push(new Tok(1, 1))
console.log(n.toks.push(new Tok(swap(n), 1)), n.toks.length)
n.toks.push(new Tok(2, 2))
console.log(n.toks.length, n.toks[0].kind)

let g = new Node(fresh())
function rebind(): number {
  g = new Node(fresh())
  return 4
}
console.log(g.toks.push(new Tok(rebind(), 2)), g.toks.length)
g.toks.push(new Tok(5, 5))
console.log(g.toks.length, g.toks[0].value)
