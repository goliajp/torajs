// then-chain links whose reaction is the only job pending run one after
// another in one job; whatever else is queued, by another chain, a
// handler's queueMicrotask, an await or a later attach, keeps its turn
function add1(v: number): number { return v + 1 }

const log: string[] = []

// alone: a long chain settles to the count of its links
let alone = Promise.resolve(0)
for (let i = 0; i < 3000; i = i + 1) {
  alone = alone.then(add1)
}

// two chains interleave link by link until the shorter one ends
let a = Promise.resolve(0)
let b = Promise.resolve(0)
for (let i = 0; i < 4; i = i + 1) {
  a = a.then((v: number) => { log.push('a' + v); return v + 1 })
}
for (let i = 0; i < 2; i = i + 1) {
  b = b.then((v: number) => { log.push('b' + v); return v + 1 })
}

// a handler queues a microtask: the next link waits behind it
let m = Promise.resolve(0)
for (let i = 0; i < 4; i = i + 1) {
  m = m.then((v: number) => {
    log.push('m' + v)
    if (v === 1) queueMicrotask(() => { log.push('q') })
    return v + 1
  })
}

// a handler attaches a second reaction to a cell further down its chain
function side(x: number): void { log.push('side' + x) }
let d = Promise.resolve(0)
let third = d
for (let i = 0; i < 4; i = i + 1) {
  d = d.then((v: number) => {
    log.push('d' + v)
    if (v === 0) third.then(side)
    return v + 1
  })
  if (i === 2) third = d
}

const n = await alone
log.push('alone' + n)
const va = await a
const vb = await b
const vm = await m
const vd = await d

// a WeakRef read inside a chain: from the first read on, every job
// boundary has kept objects to clear; a second chain beside it
const target = { n: 5 }
const ref = new WeakRef(target)
let w = Promise.resolve(0)
for (let i = 0; i < 3; i = i + 1) {
  w = w.then((v: number) => {
    const t = ref.deref()
    log.push('w' + (t === undefined ? -1 : t.n + v))
    return v + 1
  })
}
let after = Promise.resolve(0)
for (let i = 0; i < 1000; i = i + 1) {
  after = after.then(add1)
}
const vw = await w
const vafter = await after
console.log(va, vb, vm, vd, vw, vafter)
console.log(log.join(' '))
