// a method that builds an instance is inlined together with the
// constructor it reaches through the factory; field writes keep
// their frozen-object refusal and the instance keeps its identity

class P {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
  scaled(k: number): P {
    return new P(this.x * k, this.y * k)
  }
  sum(): number {
    return this.x + this.y
  }
}

let total = 0
for (let i = 0; i < 1000; i = i + 1) {
  const p = new P(i, i + 1)
  const q = p.scaled(3)
  total = total + q.sum()
}
console.log(total)

const a = new P(1, 2)
const b = a.scaled(2)
console.log(a === b, b.x, b.y, b instanceof P)

const f = Object.freeze(new P(5, 6))
try {
  ;(f as any).x = 9
  console.log("no throw", f.x)
} catch (e) {
  console.log("threw", e instanceof TypeError, f.x)
}
