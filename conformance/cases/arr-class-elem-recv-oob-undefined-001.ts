// An out-of-range read of a class-typed array is `undefined`
// (§10.4.2.1) wherever the value itself is consumed — strict
// equality, typeof, a nullish default, a truthiness test — and only
// a member access on it throws. The value lane and the receiver lane
// must agree on the same index.

class Cell {
  v: number
  constructor(v: number) {
    this.v = v
  }
}

const cells: Cell[] = [new Cell(10), new Cell(20)]

for (let i = -1; i < 4; i = i + 1) {
  const c = cells[i]
  console.log(i, c === undefined, typeof c, c ? "live" : "none")
}

console.log(cells[5] === undefined, cells[1] === undefined)
console.log((cells[7] ?? cells[0]).v)

function vOf(i: number): number {
  const c = cells[i]
  if (c === undefined) return -1
  return c.v
}
console.log(vOf(0), vOf(1), vOf(2), vOf(-3))

function vOrThrow(i: number): string {
  try {
    return String(cells[i].v)
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(vOrThrow(1), vOrThrow(2), vOrThrow(-1))
