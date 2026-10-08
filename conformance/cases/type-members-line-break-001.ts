// Type members separated only by line breaks, in an interface, a type
// alias and an inline object type; an array suffix ends at a line break.

interface Point {
  x: number
  y: number
  tags: string[]
}
type Box = {
  min: Point
  max: Point
}
function width(b: Box): number {
  return b.max.x - b.min.x
}
const p: Point = { x: 1, y: 2, tags: ["a"] }
const b: Box = { min: p, max: { x: 4, y: 6, tags: [] } }
console.log(width(b), b.min.tags.join(","))
const inline: {
  name: string
  count: number
} = { name: "n", count: 3 }
console.log(inline.name, inline.count)
let later: number[]
[1, 2].forEach((n) => console.log(n * 10))
later = [5]
console.log(later.length)
let o: { a: number }
[].forEach(() => {})
o = { a: 1 }
console.log(o.a)
