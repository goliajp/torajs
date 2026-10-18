// An array of a class that could hold its instances in place, in a
// program where an element or the array flowing into an any slot: the class keeps
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

function show(x: any): string {
  return JSON.stringify(x)
}
console.log(ts[0])
console.log(show(ts[1]))
console.log(JSON.stringify(ts))
console.log(`${ts[2]}`)
