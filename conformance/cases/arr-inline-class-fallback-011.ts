// An array of a class that could hold its instances in place, in a
// program where the array is iterated with for-of: the class keeps its
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

let s = 0
for (const t of ts) s = s + t.kind
console.log(s)
