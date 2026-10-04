// Multiplying an integer by a constant past 2^53: Number multiplication
// rounds the product, an exact integer one does not.

let x = 2 ** 53 - 1
console.log(x * 3)

let y = 9007199254740991
console.log(y * 3, y * 5 - y, -y * 3)

function triple(v: number): number {
  return v * 3
}
console.log(triple(9007199254740991), triple(3002399751580331), triple(7))

let w = 9007199254740991
const w3 = w * 3
console.log(w3, w3 + 1, w3 === 27021597764222972)

class Box {
  get v(): number {
    return 9007199254740991
  }
}
const g = new Box().v
console.log(g * 3)

const m = new Map<string, number>()
m.set('k', 4503599627370497)
console.log(m.get('k')! * 2 * 2)
