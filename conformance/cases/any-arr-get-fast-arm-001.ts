// Array<Any> reads through a double-typed index: dense slots the
// inline arm answers, and holes, sparse tails, out-of-range and
// negative indices, arguments objects and typed blocks behind an any
// view, which the runtime still answers.

const dense: any[] = [1, 'two', { three: 3 }, 4.5, null, undefined, true]
const holey: any[] = [1, , 3]
const sparse: any[] = [1, 2]
sparse[10] = 'far'
const nums: number[] = [10, 20, 30]
const viewed: any[] = nums as any[]
const out: string[] = []
let k = 0.0
for (let r = 0; r < 3; r = r + 1) {
  for (let i = 0; i < 12; i = i + 1) {
    k = i
    out.push(String(dense[k % 7]) + ':' + String(holey[k % 4]) + ':' + String(sparse[k]) + ':' + String(viewed[k % 4]))
  }
}
console.log(out.join(' '))
const neg = -1.0
console.log(dense[neg], dense[k * 100])

function args(..._xs: any[]) {
  const a: any = arguments
  let s = ''
  let j = 0.0
  for (let i = 0; i < 4; i = i + 1) {
    j = i
    s = s + String(a[j])
  }
  return s
}
console.log(args('a', 'b', 'c'))

let total = 0
const strs: any[] = []
for (let i = 0; i < 64; i = i + 1) strs.push('s' + i)
let f = 0.0
for (let i = 0; i < 100000; i = i + 1) {
  f = i % 64
  const s = strs[f]
  total = total + s.length
}
console.log(total)
