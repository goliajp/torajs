// An array of a class that could hold its instances in place, in a
// program where an element is a key of a Map: the class keeps its
// cells, and every answer is the one a cell gives.

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
