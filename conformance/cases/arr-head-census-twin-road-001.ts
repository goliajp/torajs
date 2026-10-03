// A method value rebound onto a foreign receiver runs the method's
// any-typed twin, where `this.xs[this.k]` can read Array.prototype.shift
// off a key built at run time. Calling what it read moves the deque
// head of a typed array, so every typed element read in the program
// must stay head-aware.
class Cursor {
  xs: number[]
  k: number
  constructor(xs: number[]) {
    this.xs = xs
    this.k = 0
  }
  at(): number {
    return this.xs[this.k]
  }
}

const data: number[] = [10, 20, 30, 40]
const c = new Cursor(data)
console.log(c.at())

const key = ["sh", "ift"].join("")
const foreign: any = { xs: data, k: key }
const read: any = (Cursor.prototype.at as any).call(foreign)
console.log(typeof read)
console.log(read.call(data))
console.log(data.length, data[0], data[1])
c.k = 1
console.log(c.at())

let sum = 0
for (let i = 0; i < data.length; i = i + 1) {
  sum = sum + data[i]
}
console.log(sum)
