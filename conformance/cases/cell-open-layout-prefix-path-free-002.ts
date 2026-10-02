// A class whose constructor takes an array of another class, built
// only by typed code: the constructor's boxed adapter is never
// referenced, so the elements the typed code frees keep their size.
// Beside it a wider class whose layout extends the element's by
// prefix holds an `any` in a field (the receiver-polymorphic method
// twins call its constructor with whatever the any lane hands them);
// no cell of the wider class ever lands in a slot of the element
// class, so the element frees stay sized while the wider class frees
// by the cell. Many rounds of allocation and release check both.
class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}
class Node {
  kind: number
  value: number
  left: Node | null
  right: Node | null
  constructor(kind: number, value: number, left: Node | null, right: Node | null) {
    this.kind = kind
    this.value = value
    this.left = left
    this.right = right
  }
}
class Reader {
  toks: Tok[]
  pos: number
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
  }
  sum(): Node {
    let left = this.one()
    while (this.toks[this.pos].kind === 1) {
      this.pos = this.pos + 1
      const right = this.one()
      left = new Node(1, 0, left, right)
    }
    return left
  }
  one(): Node {
    const t = this.toks[this.pos]
    this.pos = this.pos + 1
    if (t.kind === 2) return new Node(2, 0, this.one(), null)
    return new Node(0, t.value, null, null)
  }
}
function lex(n: number): Tok[] {
  const out: Tok[] = []
  for (let i = 0; i < n; i = i + 1) {
    if (i % 3 === 0) out.push(new Tok(2, 0))
    out.push(new Tok(0, i))
    out.push(new Tok(1, 0))
  }
  out.push(new Tok(0, n))
  out.push(new Tok(9, 0))
  return out
}
function walk(e: Node): number {
  if (e.kind === 0) return e.value
  if (e.kind === 2) return 0 - walk(e.left!)
  return walk(e.left!) + walk(e.right!)
}
let total = 0
let count = 0
for (let r = 0; r < 200; r = r + 1) {
  const toks = lex(5 + (r % 7))
  count = count + toks.length
  const rd = new Reader(toks)
  total = total + walk(rd.sum())
}
console.log(total, count)
const held: any = new Reader(lex(4))
console.log(walk(held.sum()))
