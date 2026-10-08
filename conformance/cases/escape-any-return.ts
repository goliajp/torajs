// A function that returns an element of a typed array whose alias
// escaped into the `any` world hands that value back as it is: its
// declared return type does not convert it.

const d: number[] = [1, 2, 3]
const x: any = d
x[1] = "s"

function get(): number {
  return d[1]
}
let b = 0
b = b + get()
console.log(b, typeof get())

function at(i: number): number {
  return d[i]
}
let c: number = 0
c = c + at(0) + at(1)
console.log(c)

function pick(i: number) {
  if (i > 5) {
    return 0
  }
  return d[i]
}
console.log(pick(1), typeof pick(1), pick(9))

function relay(i: number): number {
  const v = at(i)
  return v
}
console.log(relay(1), typeof relay(1))

function wrap(i: number): number {
  return at(i)
}
console.log(wrap(1) + "!", wrap(2) * 2)

const getLast = (): number => d[d.length - 1]
x[2] = "t"
console.log(getLast(), typeof getLast())

const words: string[] = ["a", "b"]
const w: any = words
w[0] = 0
function firstWord(): string {
  return words[0]
}
console.log(firstWord(), typeof firstWord())

function count(n: number): number {
  return n === 0 ? d[1] : count(n - 1)
}
console.log(count(3))

// a function over an array that never escaped keeps its number return
const t: number[] = [7, 8]
function tget(i: number): number {
  return t[i]
}
let u = 0
u = u + tget(0) + tget(1)
console.log(u, typeof tget(0))
