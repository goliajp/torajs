// JSON.stringify over typed class instances whose own property set
// left the layout: an expando key written through the any lane, a
// declared field hidden or turned into an accessor by defineProperty,
// next to untouched instances of the same class.
class P {
  x: number
  y: string
  constructor(x: number, y: string) {
    this.x = x
    this.y = y
  }
}

const plain = new P(1, "a")
console.log(JSON.stringify(plain))

const exp = new P(2, "b")
const ea: any = exp
ea.extra = true
console.log(JSON.stringify(exp))
console.log(JSON.stringify(exp, null, 2))

const hidden = new P(3, "c")
Object.defineProperty(hidden, "x", { enumerable: false })
console.log(JSON.stringify(hidden))

const acc = new P(4, "d")
Object.defineProperty(acc, "y", { get: () => "got", enumerable: true })
console.log(JSON.stringify(acc))

console.log(JSON.stringify([plain, exp]))
console.log(JSON.stringify(new P(5, "e")))
