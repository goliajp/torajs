// An array of a class that could hold its instances in place, in a
// program where the class has an optional field: the class keeps its
// cells, and every answer is the one a cell gives.
class Token {
  kind: number
  value?: number
  constructor(kind: number, value?: number) {
    this.kind = kind
    if (value !== undefined) this.value = value
  }
}
function make(n: number): Token[] {
  const ts: Token[] = []
  for (let i = 0; i < n; i = i + 1) ts.push(new Token(i, i % 2 === 0 ? i * 2 : undefined))
  return ts
}
const ts: Token[] = make(4)
let s = 0
for (let i = 0; i < ts.length; i = i + 1) s = s + ts[i].kind
console.log(s, ts[0].value, ts[1].value, ts[2].value)
