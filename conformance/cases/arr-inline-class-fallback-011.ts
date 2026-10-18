// An array of a class that could hold its instances in place, in a
// program where an element read by iteration or an array method: the class keeps
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

let s = 0
for (const t of ts) s = s + t.kind
console.log(s)
const p = ts.pop()
console.log(p === undefined ? -1 : p.kind, ts.length)
console.log(ts.map((t) => t.value).join(","))
