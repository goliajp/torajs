// A read of a variable or field right after a write of it takes the
// written value without going through memory, also when that value
// was picked by a conditional above the write. A second write, a write
// on only one arm, a call that writes and a loop that writes the cell
// again on its next iteration must each still be seen by the reads
// that follow them.
let g = 1

function step(x: number): number {
  g = x % 7 === 0 ? 1 : x % 7
  const a = g
  g = g + 1
  const b = g
  return a * 10 + b
}

function bump(): number {
  g = g * 2
  return g
}

function arms(x: number): number {
  g = x > 3 ? x : 3
  const a = g
  if (x % 2 === 0) g = a + 100
  const b = g
  const c = bump()
  return a * 1000000 + b * 1000 + c + g
}

function loop(n: number): number {
  let acc = 0
  for (let i = 0; i < n; i = i + 1) {
    g = (i & 1) === 0 ? i : -i
    acc = acc + g
    if (i % 3 === 0) g = g + 100
    acc = acc + g
  }
  return acc
}

class Cell {
  v: number
  constructor() {
    this.v = 0
  }
  clamp(x: number): number {
    this.v = x > 5 ? x : 5
    const a = this.v
    this.v = a + 1
    return a * 100 + this.v
  }
}

const out: number[] = []
for (let x = 0; x < 9; x = x + 1) out.push(step(x * 5), arms(x))
console.log(out.join(','))
console.log(loop(10), g)
const c = new Cell()
console.log(c.clamp(2), c.v, c.clamp(9), c.v)
