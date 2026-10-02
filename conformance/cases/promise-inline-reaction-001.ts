// The typed then / catch kernels park their reaction inside the
// derived promise. Every shape that touches that cell: then on a
// pending vs settled source, several thens on one pending source
// (registration order), a chain attached to a derived cell that is
// itself still pending, a discarded `p.then(f);`, forwarding through
// a handler of the other interest, closure handlers, a handler that
// throws, a handler that returns a promise (adoption), combinators
// over derived promises, and await on a derived promise.
function add1(v: number): number { return v + 1 }
function dbl(v: number): number { return v * 2 }
function neg(e: any): number { return -1 }

const log: string[] = []
async function later(v: number): Promise<number> {
  await Promise.resolve(0)
  await Promise.resolve(0)
  return v
}
const gate = later(10)

const d1 = gate.then(add1)
const d2 = gate.then(dbl)
const d3 = d1.then(dbl)
const d4 = d3.then((v: number): number => v + 100)
gate.then((v: number): number => { log.push("a" + v); return v })
gate.then((v: number): number => { log.push("b" + v); return v })
d2.then((v: number): number => { log.push("c" + v); return v })
console.log(await d1, await d2, await d3, await d4)
console.log(log.join(","))

const settled = Promise.resolve(5)
console.log(await settled.then(add1), await settled.then(add1).then(dbl))

const bad = Promise.reject(new Error("boom"))
const forwarded = bad.then(add1)
const caught = forwarded.catch(neg)
console.log(await caught)
const viaFulfilled = Promise.resolve(3).catch(neg).then(add1)
console.log(await viaFulfilled)
const caughtClosure = Promise.reject(new Error("x")).catch((e: any): number => (e as Error).message.length)
console.log(await caughtClosure)

const thrown = Promise.resolve(1).then((v: number): number => { throw new Error("inside " + v) })
const recovered = thrown.catch((e: any): string => "recovered: " + (e as Error).message)
console.log(await recovered)

const adopted = Promise.resolve(2).then((v: number): Promise<number> => Promise.resolve(v * 10))
console.log(await adopted)
const adoptedPending = Promise.resolve(2).then((v: number): Promise<number> => later(v + 7))
console.log(await adoptedPending)

const k = 4
const captured = Promise.resolve(1).then((v: number): number => v + k)
console.log(await captured)

const many = [Promise.resolve(1).then(add1), Promise.resolve(2).then(dbl), Promise.resolve(3).then(add1).then(dbl)]
console.log(await Promise.all(many))
console.log(await Promise.race([Promise.resolve(9).then(add1)]))
console.log(await Promise.any([Promise.reject(new Error("no")).then(add1), Promise.resolve(8).then(dbl)]))
const settledAll = await Promise.allSettled([Promise.resolve(1).then(add1), Promise.reject(new Error("r")).then(add1)])
console.log(settledAll.length, settledAll[0].status, settledAll[1].status)

async function viaAwait(v: number): Promise<number> {
  const w = await Promise.resolve(v).then(add1)
  return w * 3
}
console.log(await viaAwait(2))

let chain = Promise.resolve(0)
for (let i = 0; i < 1000; i = i + 1) {
  chain = chain.then(add1)
}
console.log(await chain)

let twice = Promise.resolve(0)
for (let i = 0; i < 300; i = i + 1) {
  twice.then(add1)
  twice = twice.then(add1)
}
console.log(await twice)
