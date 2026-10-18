// A top-level array that holds its class in place as the receiver of
// a push, from the top level and from a function; and a push whose
// argument rebinds the receiver's binding while it runs, which appends
// to the array the binding held before the arguments ran.

class Cell {
  v: number
  w: number
  constructor(v: number, w: number) {
    this.v = v
    this.w = w
  }
}

const all: Cell[] = []
function add(v: number): void {
  all.push(new Cell(v, v * v))
}
for (let i = 0; i < 20; i = i + 1) add(i)
all.push(new Cell(100, 0))
let s = 0
for (let i = 0; i < all.length; i = i + 1) s = s + all[i].v + all[i].w
console.log(all.length, s)

let cur: Cell[] = []
let old: Cell[] = cur
function swap(): number {
  cur = []
  return 7
}
cur.push(new Cell(1, 2))
console.log(cur.push(new Cell(swap(), 3)), cur.length, old.length, old[1].v)

let nums: number[] = [1]
let numsOld: number[] = nums
function reset(): number {
  nums = []
  return 5
}
console.log(nums.push(reset()), nums.length, numsOld.length, numsOld[1])

// the same with nothing else holding the array the push appends to
let lone: Cell[] = []
lone.push(new Cell(0, 0))
function swapLone(): number {
  lone = []
  return 3
}
console.log(lone.push(new Cell(swapLone(), 1)), lone.length)
let loneNums: number[] = [0]
function resetLone(): number {
  loneNums = []
  return 4
}
console.log(loneNums.push(resetLone()), loneNums.length)
