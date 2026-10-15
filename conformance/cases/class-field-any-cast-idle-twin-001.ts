// A method read off the prototype runs on a foreign receiver: its any
// twin builds the node from dynamic values, so the node's number
// fields keep fractions (the nested node is copied from a dynamic object).
class Node1 {
  kind: number
  left: Node1 | null
  constructor(kind: number, left: Node1 | null) {
    this.kind = kind
    this.left = left
  }
}

class Builder {
  k: number
  constructor(k: number) {
    this.k = k
  }
  child(): Node1 | null {
    return null
  }
  make(): Node1 {
    return new Node1(this.k, this.child())
  }
}

const typed = new Builder(3).make()
console.log(typed.kind, typed.left === null)

const fake: any = {
  k: 2,
  child() {
    return { kind: 1.25, left: null }
  },
}
const make = Builder.prototype.make
const n = make.call(fake)
console.log(n.kind, n.left!.kind)
let sum = 0
for (let i = 0; i < 4; i++) sum += new Node1(i, null).kind / 2
console.log(sum)
