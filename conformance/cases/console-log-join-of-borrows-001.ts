// A `?:` / `||` / `??` whose arms are all borrowed (an array element,
// a binding, a string literal) answers a borrow: printing it must not
// release the element, which its array still holds and prints again
// after a few fresh strings were built.
const m = 'foo123 bar456'.match(/(\d+)/)
console.log(m === null ? 'null' : m[0])
const xs = ['a'.repeat(2), 'b'.repeat(2)]
console.log(xs.length > 5 ? 'none' : xs[1])
console.log(xs[0] || 'empty')
const ys: (string | undefined)[] = ['c'.repeat(3), undefined]
console.log(ys[0] ?? 'missing', ys[1] ?? 'missing')
const pad = ['x'.repeat(2), 'y'.repeat(2), 'z'.repeat(2)]
console.log(m === null ? 'null' : m[1], xs[0], xs[1], ys[0], pad.join(''))

function show(arr: string[], k: number): number {
  console.log(k > 0 ? arr[k] : arr[0])
  console.log(arr[k] || 'none')
  return arr.length
}
const zs = ['p'.repeat(2), 'q'.repeat(2)]
show(zs, 1)
show(zs, 0)
const fresh = ['r'.repeat(2), 's'.repeat(2)]
console.log(zs[0], zs[1], fresh.join('+'))
