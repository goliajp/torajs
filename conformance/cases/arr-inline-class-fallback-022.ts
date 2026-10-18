// An array of a class that could hold its instances in place, in a
// program where the array is frozen: the class keeps its cells, and
// every answer is the one a cell gives.

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

Object.freeze(ts)
try {
  ts.push(new Token(5, 5))
  console.log("pushed", ts.length)
} catch (e) {
  console.log("array", e instanceof TypeError, ts.length)
}
