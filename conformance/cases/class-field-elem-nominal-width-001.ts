// An instance read back as an element of an array held in another
// class's field: the field it reads has the instance class's layout
// width, here a double past the int64 range.
class Token {
  value: number
  constructor(value: number) {
    this.value = value
  }
}

class Node2 {
  value: number
  constructor(value: number) {
    this.value = value
  }
}

function grow(n: number): number {
  let v = 0
  for (let i = 0; i < n; i++) v = v * 10 + 9
  return v
}

class Holder {
  toks: Token[]
  pos: number
  constructor(toks: Token[]) {
    this.toks = toks
    this.pos = 0
  }
  node(): Node2 {
    const t = this.toks[this.pos]
    return new Node2(t.value)
  }
  raw(): number {
    const t = this.toks[this.pos]
    return t.value
  }
}

function viaParam(h: Holder): Node2 {
  const t = h.toks[0]
  return new Node2(t.value)
}

const h = new Holder([new Token(grow(25)), new Token(7)])
console.log(h.node().value, h.raw(), viaParam(h).value)
h.pos = 1
console.log(h.node().value, h.raw())
