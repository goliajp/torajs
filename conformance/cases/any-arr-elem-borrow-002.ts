// An `any[]` element read keeps its own share whenever something
// between the read and the element's use can drop the slot: a
// function the loop calls rewrites, empties or splices the array, or
// a generator is suspended at a `yield` while the element is bound
// and the caller rewrites the array before resuming it. The first
// loop has no such call and is the one whose element is borrowed.

function fill(a: any[], tag: string): void {
  for (let j = 0; j < 8; j = j + 1) a.push('item' + j + tag)
}

function overwrite(a: any[]): void {
  for (let k = 0; k < a.length; k = k + 1) a[k] = 'w' + k
}

function emptyAndRefill(a: any[]): void {
  a.length = 0
  for (let k = 0; k < 8; k = k + 1) a.push('v' + k + k)
}

function spliceAndRefill(a: any[]): void {
  a.splice(0, a.length)
  for (let k = 0; k < 8; k = k + 1) a.push('s' + k)
}

const arr: any[] = []
fill(arr, '')
let marker = 0
let plain = 0
for (let i = 0; i < 1000; i = i + 1) {
  const s = arr[i % 8]
  if (i === 500) marker = marker + 1
  plain = plain + s.length
}
console.log(plain, marker)

let overwritten = 0
for (let i = 0; i < 1000; i = i + 1) {
  const s = arr[i % 8]
  if (i === 500) overwrite(arr)
  overwritten = overwritten + s.length
}
console.log(overwritten, arr[0])

const emptied: any[] = []
fill(emptied, '-e')
let afterEmpty = 0
for (let i = 0; i < 1000; i = i + 1) {
  const s = emptied[i % 8]
  if (i === 500) emptyAndRefill(emptied)
  afterEmpty = afterEmpty + s.length
}
console.log(afterEmpty, emptied[0])

const spliced: any[] = []
fill(spliced, '-sp')
let afterSplice = 0
for (let i = 0; i < 1000; i = i + 1) {
  const s = spliced[i % 8]
  if (i === 500) spliceAndRefill(spliced)
  afterSplice = afterSplice + s.length
}
console.log(afterSplice, spliced[7])

// the generator holds the element across a yield; the caller
// replaces every slot before resuming it
function* walk(a: any[]): Generator<number> {
  let acc = 0
  for (let i = 0; i < 16; i = i + 1) {
    const s = a[i % 8]
    yield acc
    acc = acc + s.length
  }
}
const gs: any[] = []
fill(gs, '-generated')
let resumed = ''
let round = 0
for (const n of walk(gs)) {
  resumed = resumed + n + ','
  for (let k = 0; k < gs.length; k = k + 1) gs[k] = 'r' + round + k
  round = round + 1
}
console.log(resumed, gs[0])
