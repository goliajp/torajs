// An array of a class that could hold its instances in place, in a
// program where an element used as a key of a keyed collection or searched for: the class keeps
// its cells, and every answer is the one a cell gives.

class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

function make(n: number): Token[] {
  const ts: Token[] = []
  for (let i = 0; i < n; i = i + 1) ts.push(new Token(i, i * 2))
  return ts
}

const ts: Token[] = make(4)

const m = new Map<Token, number>()
m.set(ts[0], 1)
console.log(m.get(ts[0]), m.has(ts[1]))
const s = new Set<Token>(ts)
console.log(s.has(ts[2]), s.size)
const t = ts[3]
console.log(ts.indexOf(t), ts.includes(ts[1]))
const w = new WeakMap<Token, string>()
w.set(ts[1], "one")
console.log(w.get(ts[1]))
