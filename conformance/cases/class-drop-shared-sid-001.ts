// Two classes with the same field list intern onto one struct layout
// while each keeps its own runtime tag. The shared release fn must
// answer both tags, so instances of either class die through it with
// their heap child released, and `instanceof` still tells them apart.
class Leaf {
  v: number
  constructor(v: number) { this.v = v }
}
class Alpha {
  k = 0
  link: Leaf | null = null
}
class Beta {
  k = 0
  link: Leaf | null = null
}
let total = 0
let alphas = 0
let betas = 0
for (let i = 0; i < 3000; i++) {
  if (i % 3 === 0) {
    const a = new Alpha()
    a.k = i
    a.link = new Leaf(i)
    total += a.k + a.link.v
    if (a instanceof Alpha) alphas++
  } else {
    const b = new Beta()
    b.k = i
    b.link = new Leaf(i * 2)
    total += b.k + b.link.v
    if (b instanceof Beta) betas++
  }
}
console.log(total, alphas, betas)
const a = new Alpha()
const b = new Beta()
console.log(a instanceof Beta, b instanceof Alpha, a.link === null, b.link === null)
