// An array of a class that could hold its instances in place, in a
// program where two views of one element are compared by identity: the
// class keeps its cells, and every answer is the one a cell gives.

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

const a = ts[0]
const b = ts[0]
console.log(a === b)
