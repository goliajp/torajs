// An `any[]` element read whose box is only inspected while the array
// stays untouched keeps no share of its own. The shapes that must
// still release correctly: the slot overwritten by a Proxy `length`
// trap while the element is in use, the slot overwritten after the
// use, immediates and holes on the slow arm, and loops left early by
// `break` or a throw while the element is bound.

const arr: any[] = []
for (let j = 0; j < 8; j = j + 1) arr.push('item' + j)

// plain: the length of every element, no writes in the loop
let total = 0
for (let i = 0; i < 1000; i = i + 1) {
  const s = arr[i % 8]
  total = total + s.length
}
console.log(total)

// the length trap drops the receiver out of the array while the
// read is still using it: the slot held the only share. The trap
// reaches the array through its target and counts its calls in
// arr[1], so the loop body itself writes nothing.
function makeTrap(holder: any): any {
  return new Proxy(holder, {
    get(t: any, k: any) {
      if (k === 'length') {
        t.a[1] = t.a[1] + 'x'
        t.a[0] = t.a[1].length < 8 ? makeTrap(t) : null
        return 99
      }
      return undefined
    },
  })
}
arr[0] = makeTrap({ a: arr })
let viaTrap = 0
for (let i = 0; i < 6; i = i + 1) {
  const s = arr[i % 2]
  viaTrap = viaTrap + s.length
}
console.log(viaTrap, arr[1], arr[0])
arr[0] = 'item0'
arr[1] = 'item1'

// the slot is overwritten after the element was measured
let rewritten = 0
for (let i = 0; i < 16; i = i + 1) {
  const s = arr[i % 8]
  rewritten = rewritten + s.length
  arr[i % 8] = 'x' + s
}
console.log(rewritten, arr[3])

// immediates and a hole take the slow arm, as do indices past the end
const mixed: any[] = [1, 'ab', true, , 'cdef', null]
let seen = ''
for (let i = 0; i < 8; i = i + 1) {
  const e = mixed[i]
  seen = seen + String(e === null || e === undefined ? e : e.length) + ','
}
console.log(seen)

// leaving the loop while the element is bound
let untilLong = 0
for (let i = 0; i < 16; i = i + 1) {
  const s = arr[i % 8]
  if (i >= 5 && s.length > 6) break
  untilLong = untilLong + 1
}
console.log(untilLong)

let beforeThrow = 0
try {
  for (let i = 0; i < 16; i = i + 1) {
    const s = arr[i % 8]
    if (i >= 3 && s.length > 6) throw new Error('long at ' + i)
    beforeThrow = beforeThrow + 1
  }
} catch (e: any) {
  console.log(e.message, beforeThrow)
}

// a null receiver on the slow arm throws while the element is bound
const holey: any[] = ['a', null, 'c']
let caught = ''
try {
  for (let i = 0; i < 3; i = i + 1) {
    const s = holey[i]
    caught = caught + s.length
  }
} catch (e: any) {
  caught = caught + '!'
}
console.log(caught)
