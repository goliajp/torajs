// A push whose argument overwrites the place the receiver was read
// from, or drops the object holding it: the element goes to the array
// read before the arguments ran (§13.3.6.1), for arrays of objects,
// strings, numbers and `any`.

class P {
  name: string
  constructor(name: string) {
    this.name = name
  }
}
class Box {
  items: P[]
  words: string[]
  nums: number[]
  anys: any[]
  constructor() {
    this.items = []
    this.words = []
    this.nums = []
    this.anys = []
  }
}

const b = new Box()
function wipe(x: Box): string {
  x.items = []
  x.words = []
  x.nums = []
  x.anys = []
  return "w" + String(x.items.length)
}
b.items.push(new P("a"))
console.log(b.items.push(new P(wipe(b))), b.items.length)
b.words.push("a")
console.log(b.words.push(wipe(b)), b.words.length)
b.nums.push(1)
console.log(b.nums.push(wipe(b).length), b.nums.length)
b.anys.push(1)
console.log(b.anys.push(wipe(b)), b.anys.length)

let cur = new Box()
function replace(): string {
  cur = new Box()
  return "r"
}
cur.items.push(new P("x"))
console.log(cur.items.push(new P(replace())), cur.items.length)
cur.words.push("x")
console.log(cur.words.push(replace()), cur.words.length)

const grid: string[][] = [["a"], ["b"]]
function reset(): string {
  grid[0] = []
  return "z"
}
console.log(grid[0].push(reset()), grid[0].length, grid[1].length)
