// `xs[i] = s` where `s` is a block-local `const` string read only by
// the rest of its block hands the slot the binding's own reference.
// Every shape here either takes that hand-over or must refuse it; the
// output is the same either way, and a wrong hand-over shows up as a
// freed string read back (garbage / crash) or a leak under the
// allocator checks.
const pre = '  Hello42 word  '
const tail = process.argv.length > 99 ? 'never' : 'x'

// the plain shape: ring store, then reads of `s`
const ring: string[] = ['', '', '', '']
let total = 0
for (let i = 0; i < 40; i = i + 1) {
  const s = pre + tail + i
  ring[i & 3] = s
  total = total + s.charCodeAt(i & 15) + s.length
  const u = s + i
  total = total + u.charCodeAt(u.length - 1)
  if (s.length > 18) {
    const k = s.charCodeAt(0)
    total = total + k
  }
}
console.log('ring', total, ring.join('|'))

// out-of-bounds stores grow the array through the kernel
const grown: string[] = []
let glen = 0
for (let i = 0; i < 20; i = i + 1) {
  const s = tail + i + pre
  grown[i] = s
  glen = glen + s.length
}
console.log('grow', glen, grown.length, grown[0], grown[19])

// a later element store to the same slot must not release `s` under its reads
const over: string[] = ['a', 'b']
let o2 = 0
for (let i = 0; i < 6; i = i + 1) {
  const s = pre + i
  over[i & 1] = s
  over[i & 1] = 'y'
  o2 = o2 + s.charCodeAt(2) + s.length
}
console.log('overwrite', o2, over.join(','))

// `s` handed to a call, stored elsewhere, or kept by another array
function keepLen(v: string): number {
  return v.length
}
const other: string[] = ['', '']
const firsts: string[] = []
let o3 = 0
for (let i = 0; i < 6; i = i + 1) {
  const s = tail + pre + i
  over[i & 1] = s
  o3 = o3 + keepLen(s)
  other[i & 1] = s
  firsts.push(s)
  o3 = o3 + s.length
}
console.log('shared', o3, over.join(','), other.join(','), firsts.length, firsts[5])

// returned after the store
function build(xs: string[], n: number): string {
  {
    const s = pre + n
    xs[0] = s
    return s
  }
}
const holder: string[] = ['']
const r = build(holder, 7)
console.log('returned', r, holder[0], r === holder[0])

// a frozen receiver throws at the store; the binding still owns `s`
const frozen: string[] = ['f0', 'f1']
Object.freeze(frozen)
const kept: string[] = []
let thrown = 0
for (let j = 0; j < 5; j = j + 1) {
  try {
    for (let i = j; i < 9; i = i + 1) {
      const s = pre + 'frozen' + i
      kept.push(s)
      frozen[i & 1] = s
      total = total + s.length
    }
  } catch (e) {
    thrown = thrown + (e instanceof TypeError ? 1 : 100)
  }
}
console.log('frozen', thrown, frozen.join(','), kept.join('|'))

// a sealed receiver takes the in-bounds store through its guard call
const sealed: string[] = ['s0', 's1']
Object.seal(sealed)
let sl = 0
for (let i = 0; i < 6; i = i + 1) {
  const s = pre + 'sealed' + i
  sealed[i & 1] = s
  sl = sl + s.charCodeAt(3) + s.length
}
console.log('sealed', sl, sealed.join(','))

// the slot keeps `s` alive past the block that made it
const late: string[] = ['', '', '']
for (let i = 0; i < 3; i = i + 1) {
  const s = pre.trim() + '#' + i
  late[i] = s
  if (i === 1) {
    continue
  }
  total = total + s.length
}
console.log('late', late.join(','), total)

// the same shapes inside a generator, an async function and an arrow
function* gen(xs: string[]): Generator<number> {
  for (let i = 0; i < 4; i = i + 1) {
    const s = pre + 'gen' + i
    xs[i & 1] = s
    const k = s.length + s.charCodeAt(2)
    yield k
  }
}
const gx: string[] = ['', '']
let gsum = 0
for (const k of gen(gx)) {
  gsum = gsum + k
}
console.log('generator', gsum, gx.join(','))

async function later(xs: string[]): Promise<number> {
  let acc = 0
  for (let i = 0; i < 4; i = i + 1) {
    const s = pre + 'async' + i
    xs[i & 1] = s
    acc = acc + s.length
  }
  await null
  for (let i = 0; i < 2; i = i + 1) {
    const s = xs[i] + 'tail'
    xs[i] = s
    acc = acc + s.charCodeAt(s.length - 1)
  }
  return acc
}
const ax: string[] = ['', '']
later(ax).then((v) => console.log('async', v, ax.join(',')))

const arrowStore = (xs: string[], n: number): number => {
  let acc = 0
  for (let i = 0; i < n; i = i + 1) {
    const s = pre + 'arrow' + i
    xs[i % 3] = s
    acc = acc + s.length
  }
  return acc
}
const rx: string[] = ['', '', '']
console.log('arrow', arrowStore(rx, 7), rx.join(','))
