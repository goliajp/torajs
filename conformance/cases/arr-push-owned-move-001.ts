// every push argument shape whose own reference the array slot takes
// over, next to borrow shapes of the same values, read back after the
// sources are gone
class C { constructor(public v: number) {} }
function mk(i: number): C { return new C(i * 2) }
function hello(): string { return 'hi' }
const n = parseInt(process.argv[2] ?? '50', 10)
const cs: C[] = []
const ss: string[] = []
const fs: (() => number)[] = []
const ns: number[][] = []
const os: { a: number }[] = []
const bs: bigint[] = []
const gs: (() => string)[] = []
let shared = new C(-1)
let a = 'x'
for (let i = 0; i < n; i = i + 1) {
  const b = 'y' + i
  cs.push(new C(i))
  cs.push(mk(i))
  cs.push(shared)
  ss.push(a + b)
  ss.push(b)
  ss.push(String(i))
  const k = i
  fs.push(() => k + 1)
  ns.push([])
  ns.push([i, i + 1])
  os.push({ a: i })
  bs.push(2n)
  bs.push(BigInt(i) * 3n)
  gs.push(hello)
  if (i % 10 === 9) shared = new C(i)
  if (i % 7 === 0) cs.unshift(new C(1000 + i))
}
ns[0].push(7)
let acc = 0
for (let i = 0; i < cs.length; i = i + 1) acc = acc + cs[i].v
let fsum = 0
for (let i = 0; i < fs.length; i = i + 1) fsum = fsum + fs[i]()
let nsum = 0
for (let i = 0; i < ns.length; i = i + 1) nsum = nsum + ns[i].length
let osum = 0
for (let i = 0; i < os.length; i = i + 1) osum = osum + os[i].a
let bsum = 0n
for (let i = 0; i < bs.length; i = i + 1) bsum = bsum + bs[i]
console.log(cs.length, acc, ss.length, ss.join('').length, ss[ss.length - 3])
console.log(fs.length, fsum, ns.length, nsum, os.length, osum, bs.length, bsum)
console.log(gs.length, gs[0](), gs[gs.length - 1]())
const frozen: C[] = [new C(1)]
Object.freeze(frozen)
let caught = 0
for (let i = 0; i < 5; i = i + 1) {
  try {
    frozen.push(new C(i))
  } catch (e) {
    caught = caught + 1
    if (i === 0) console.log((e as Error).name, (e as Error).message)
  }
}
console.log('frozen', caught, frozen.length, frozen[0].v)
