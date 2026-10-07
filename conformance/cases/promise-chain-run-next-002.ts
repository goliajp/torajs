// job order around then-chain links that may run in their source's job:
// async functions awaiting beside a chain, a handler that awaits, a
// handler throwing mid-chain, finally, a handler returning a promise,
// a reaction attached to a settled promise from inside a handler, and
// microtasks queued before the chain starts
function inc(v: number): number { return v + 1 }

const log: string[] = []

// microtasks queued ahead of a chain keep their place
queueMicrotask(() => { log.push('q0') })
let c = Promise.resolve(0)
for (let i = 0; i < 3; i = i + 1) {
  c = c.then((v: number) => { log.push('c' + v); return v + 1 })
}
queueMicrotask(() => { log.push('q1') })

// an async function awaiting beside a chain
async function steps(tag: string, n: number): Promise<number> {
  let s = 0
  for (let i = 0; i < n; i = i + 1) {
    await null
    log.push(tag + i)
    s = s + i
  }
  return s
}
const sp = steps('s', 4)
let e = Promise.resolve(10)
for (let i = 0; i < 4; i = i + 1) {
  e = e.then((v: number) => { log.push('e' + v); return v + 1 })
}

// a handler throwing mid-chain: the rest forwards the rejection
let t = Promise.resolve(0)
for (let i = 0; i < 5; i = i + 1) {
  t = t.then((v: number) => {
    log.push('t' + v)
    if (v === 2) throw new Error('boom')
    return v + 1
  })
}
const tc = t.catch((err: any) => { log.push('caught ' + err.message); return -1 })

// finally inside a chain
let f = Promise.resolve(1)
f = f.then(inc)
f = f.finally(() => { log.push('fin') })
f = f.then((v: number) => { log.push('f' + v); return v + 1 })

// a handler returning a promise is adopted; its own log only, the
// chains beside it keep theirs
const rlog: string[] = []
let r = Promise.resolve(0)
r = r.then((v: number) => { rlog.push('r' + v); return Promise.resolve(v + 100) })
r = r.then((v: number) => { rlog.push('r' + v); return v + 1 })
let g = Promise.resolve(0)
for (let i = 0; i < 4; i = i + 1) {
  g = g.then((v: number) => { log.push('g' + v); return v + 1 })
}

// a handler attaching to a settled promise queues that reaction first
const done = Promise.resolve(7)
let h = Promise.resolve(0)
for (let i = 0; i < 3; i = i + 1) {
  h = h.then((v: number) => {
    log.push('h' + v)
    if (v === 0) done.then((x: number) => { log.push('done' + x) })
    return v + 1
  })
}

const vs = await sp
const vc = await c
const ve = await e
const vt = await tc
const vf = await f
const vr = await r
const vg = await g
const vh = await h
console.log(vs, vc, ve, vt, vf, vr, vg, vh)
console.log(log.join(' '))
console.log(rlog.join(' '))

// a second round after the top-level awaits: an async handler in a chain
const log2: string[] = []
async function slow(v: number): Promise<number> {
  await null
  log2.push('slow' + v)
  return v * 2
}
let k = Promise.resolve(1)
k = k.then((v: number) => { log2.push('k' + v); return v + 1 })
k = k.then(slow)
k = k.then((v: number) => { log2.push('k' + v); return v + 1 })
let l = Promise.resolve(0)
for (let i = 0; i < 6; i = i + 1) {
  l = l.then((v: number) => { log2.push('l' + v); return v + 1 })
}
const vk = await k
const vl = await l
console.log(vk, vl)
console.log(log2.join(' '))
