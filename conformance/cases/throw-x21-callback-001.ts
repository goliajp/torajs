// The runtime calls back into user code (a sort comparator, an iterator's
// next, an array callback) that itself calls throwing functions; the
// runtime's registers survive, and a throw out of the callback reaches
// the caller's catch.
function key(n: number): number {
  if (n === 13) throw new Error('unlucky')
  return n % 7
}

function cmp(a: number, b: number): number {
  let r = 0
  try {
    r = key(a) - key(b)
  } catch (e) {
    r = a === 13 ? 1 : -1
  }
  return r !== 0 ? r : a - b
}

const xs: number[] = []
for (let i = 0; i < 40; i++) xs.push((i * 17) % 41)
xs.sort(cmp)
console.log(xs.join(','))

const ys = [5, 13, 2]
try {
  ys.sort((a, b) => key(a) - key(b))
  console.log('sorted', ys.join(','))
} catch (e) {
  console.log('sort threw', (e as Error).message)
}

class Counter {
  i = 0
  limit: number
  constructor(limit: number) {
    this.limit = limit
  }
  [Symbol.iterator]() {
    return this
  }
  next(): { value: number; done: boolean } {
    this.i++
    if (this.i > this.limit) return { value: 0, done: true }
    return { value: key(this.i), done: false }
  }
}

let acc = 0
for (const v of new Counter(10)) acc += v
console.log('iter', acc)
try {
  for (const v of new Counter(20)) acc += v
} catch (e) {
  console.log('iter threw', (e as Error).message, acc)
}

const mapped = [1, 2, 3, 4].map((n) => key(n) * 2)
console.log(mapped.join(','))
try {
  console.log([12, 13, 14].map((n) => key(n)).join(','))
} catch (e) {
  console.log('map threw', (e as Error).message)
}
console.log(key(20) + key(21))
