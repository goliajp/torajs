// An array's buffer grows in steps as it is pushed to. Whatever the
// steps are, every element must survive each move of the buffer: for
// a typed array, an array of objects, an array held as any, one that
// is shifted between pushes (the head offset is folded on the next
// grow), one written past its end by index, and one grown from a
// literal's inline slots.
class P {
  n: number
  constructor(n: number) {
    this.n = n
  }
}

function checksum(xs: number[]): number {
  let s = 0
  for (let i = 0; i < xs.length; i = i + 1) s = (s * 31 + xs[i]) % 1000003
  return s
}

const sizes = [0, 1, 3, 4, 5, 8, 15, 16, 17, 31, 32, 33, 64, 65, 127, 128, 129, 300]
const sums: number[] = []
for (let k = 0; k < sizes.length; k = k + 1) {
  const xs: number[] = []
  const ps: P[] = []
  const anys: any[] = []
  for (let i = 0; i < sizes[k]; i = i + 1) {
    xs.push(i * 7 + 1)
    ps.push(new P(i))
    anys.push(i % 3 === 0 ? 'v' + i : i)
  }
  let pn = 0
  for (let i = 0; i < ps.length; i = i + 1) pn = pn + ps[i].n
  let an = 0
  for (let i = 0; i < anys.length; i = i + 1) an = an + (typeof anys[i] === 'string' ? anys[i].length : anys[i])
  sums.push(xs.length, checksum(xs), pn, an)
}
console.log(sums.join(','))

// shift between pushes: a queue that never holds more than a few
const q: number[] = []
let drained = 0
for (let i = 0; i < 200; i = i + 1) {
  q.push(i)
  q.push(i * 2)
  if (i % 3 !== 0) drained = drained + q.shift()!
}
console.log(q.length, q[0], q[q.length - 1], drained, checksum(q))

// a literal's inline slots, then pushes
const lit = [10, 20, 30]
for (let i = 0; i < 40; i = i + 1) lit.push(i)
console.log(lit.length, lit[0], lit[2], lit[3], lit[42], checksum(lit))

// a store past the end by index
const sparse: any[] = [1, 2]
sparse[20] = 'x'
sparse.push('y')
console.log(sparse.length, sparse[1], sparse[2], sparse[20], sparse[21])

// one array appended to another in bulk
const big: number[] = [1, 2, 3]
const more: number[] = []
for (let i = 0; i < 50; i = i + 1) more.push(i)
big.push(...more)
console.log(big.length, big[3], big[52], checksum(big.concat(more)))
