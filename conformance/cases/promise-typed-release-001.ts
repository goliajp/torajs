// Typed promise releases: a reference that leaves the cell alive, the
// last reference, and cells that may close a ring (an expando bag, a
// settled heap value, a closure handler) — each in a reassigned slot,
// with the microtask order around them printed.

function add1(v: number): number {
  return v + 1
}

const log: string[] = []

// shared: the slot's old value stays alive through `keep`
let p = Promise.resolve(1)
const keep = p
for (let i = 0; i < 4; i = i + 1) {
  p = p.then(add1)
}
queueMicrotask(() => log.push("tick"))

// last reference: each old link dies when the slot moves on
let q = Promise.resolve(10)
q = q.then(add1)
q = q.then(add1)

// settled heap value, shared then released
let h = Promise.resolve([1, 2, 3])
const h2 = h
h = h.then((a: number[]) => a.concat([4]))

// expando bag on a promise whose slot is reassigned
let b = Promise.resolve(7)
Object.defineProperty(b, "tag", { value: "bag" })
const b2 = b
b = b.then(add1)

// closure handler capturing the slot it replaces
let c = Promise.resolve(2)
const k = 5
c = c.then((v: number) => v * k)

// a rejected link, handled, released
let r: Promise<number> = Promise.reject(new Error("boom"))
r = r.catch((e: any) => {
  log.push("caught " + e.message)
  return -1
})

console.log(await p, await keep)
console.log(await q)
console.log((await h).length, (await h2).length)
console.log(await b, (b2 as any).tag)
console.log(await c)
console.log(await r)
console.log(log.join(","))
