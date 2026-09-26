// Retains of values that may be NaN-box immediates: every encoding an
// `any` slot can hold goes through an indexed read, a closure capture
// and a defineProperty value, many times over.

const heap = 'heap-' + 'string-' + 'value'
const obj = { tag: 'o' }
const mixed: any[] = [7, 2.5, 'ab', heap, 'static-literal', null, undefined, true, obj, -0, NaN]
let reads = 0
let lens = 0
for (let i = 0; i < 110000; i = i + 1) {
  const v = mixed[i % 11]
  if (typeof v === 'string') lens = lens + v.length
  reads = reads + 1
}
console.log(reads, lens)

const keep: (() => any)[] = []
for (let i = 0; i < 11; i = i + 1) {
  let cap: any = mixed[i]
  keep.push(() => cap)
}
let shown = ''
for (let r = 0; r < 1000; r = r + 1) {
  for (const f of keep) {
    const got = f()
    if (r === 999) shown = shown + String(got) + '|'
  }
}
console.log(shown)

const target: any = {}
for (let i = 0; i < 11; i = i + 1) {
  var val: any = mixed[i]
  Object.defineProperty(target, 'p' + i, { value: val, enumerable: true })
}
console.log(Object.keys(target).length, target.p3, target.p8.tag, target.p5)
mixed.length = 0
console.log(heap.length, obj.tag, keep.length)
