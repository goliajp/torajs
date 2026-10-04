// Objects created and released inside a loop body, compared by
// identity, kept past the iteration through an array, and created on
// one branch but read after the join.
class P {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
  sum(): number {
    return this.x + this.y
  }
  moved(d: number): P {
    return new P(this.x + d, this.y + d)
  }
}

let same = 0
let diff = 0
for (let i = 0; i < 1000; i = i + 1) {
  const a = new P(i, i)
  const b = new P(i, i)
  const c = a
  if (a === b) diff = diff + 1
  if (a === c) same = same + 1
  if (a !== b) same = same + 1
}
console.log("identity", same, diff)

const kept: P[] = []
let acc = 0
for (let i = 0; i < 100; i = i + 1) {
  const p = new P(i, 2 * i)
  const q = p.moved(1)
  if (i % 10 === 0) kept.push(q)
  acc = acc + q.sum()
}
let keptSum = 0
for (const k of kept) keptSum = keptSum + k.x * 1000 + k.y
console.log("kept", kept.length, keptSum, acc)

let joined = 0
for (let i = 0; i < 200; i = i + 1) {
  let p: P | null = null
  if (i % 3 === 0) {
    p = new P(i, 1)
  } else if (i % 3 === 1) {
    p = new P(1, i).moved(2)
  }
  if (p !== null) joined = joined + p.sum()
  else joined = joined - 1
}
console.log("joined", joined)

let last: P | null = null
let chain = 0
for (let i = 0; i < 50; i = i + 1) {
  const n = new P(i, i + 1)
  if (last !== null) chain = chain + last.x - n.y
  last = n
}
console.log("chain", chain, last === null ? -1 : last.sum())
