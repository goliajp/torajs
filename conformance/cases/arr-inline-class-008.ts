// An array of a class held in place, next to an array of the class the
// Array constructor makes with holes: reading a field of a hole raises
// the TypeError a missing element raises.

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

const empty = new Array<Token>(3)
console.log(empty.length, ts[1].kind)
try {
  console.log(empty[0].kind)
} catch (e) {
  console.log("caught", e instanceof TypeError)
}
