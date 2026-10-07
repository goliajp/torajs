// every cell of a chain kept from outside while its links run one after
// another in one job: each one still reads its own value afterwards, and
// a later reaction on any of them runs in attach order
const cells: Promise<number>[] = []
let p = Promise.resolve(1)
cells.push(p)
for (let i = 0; i < 200; i = i + 1) {
  p = p.then((v: number) => v * 3 % 1000003)
  cells.push(p)
}
const last = await p

const seen: number[] = []
let sum = 0
for (let i = 0; i < cells.length; i = i + 1) {
  cells[i].then((v: number) => {
    seen.push(i)
    sum = (sum + v) % 1000003
    return v
  })
}
await Promise.resolve(0)
let inOrder = true
for (let i = 0; i < seen.length; i = i + 1) {
  if (seen[i] !== i) inOrder = false
}
console.log(last, seen.length, sum, inOrder)

// a chain whose cells nobody keeps, beside one whose every cell is kept
let free = Promise.resolve(0)
const kept: Promise<number>[] = []
let held = Promise.resolve(0)
for (let i = 0; i < 50; i = i + 1) {
  free = free.then((v: number) => v + 2)
  held = held.then((v: number) => v + 5)
  kept.push(held)
}
const vfree = await free
const vheld = await held
const mid = await kept[24]
console.log(vfree, vheld, mid, kept.length)
