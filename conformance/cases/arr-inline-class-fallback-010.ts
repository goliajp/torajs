// An array of a class that could hold its instances in place, in a
// program where a push between a view's binding and its read: the class keeps
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

const t = ts[0]
ts.push(new Token(10, 20))
for (let i = 0; i < 40; i = i + 1) ts.push(new Token(i, i))
console.log(t.kind, t.value, ts.length)
