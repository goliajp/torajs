// A pending promise's reactions are edges the cycle collector walks:
// a derived promise parked in its source's list, the closure env its
// own reaction calls, and what an `any`-receiver `.then` record holds.
// While a reaction is parked, the derived promise's pointer back to
// its source is not a reference, so a reaction on a promise nobody
// can settle any more goes away with that promise.
//
// Observable here: the rings and the dropped reactions do not crash
// the walk or the teardown, chains that settle still settle in order,
// and a live ring still runs once its source settles. Reclamation is
// a peak-RSS question, measured out of band: the loop below at 1e3
// and at 1e5 iterations peaks at the same resident size.

function never(): Promise<number> {
  return new Promise<number>(() => {})
}

let made = 0
for (let i = 0; i < 300; i++) {
  // a reaction on a promise nobody can settle
  never().then((x: number) => x + 1)
  // an object holding a reaction whose env holds the object
  const o: any = { i: i }
  o.p = new Promise(() => {}).then(() => o)
  // a derived promise whose own handler names it
  const p = never()
  const d: Promise<any> = p.then((): any => d)
  // a source whose handler names the source, through the any lane
  const s: any = never()
  s.then(() => s)
  // two links, the second one's env holding the holder
  const h: any = { i: i }
  h.p = never().then((x: number) => x + 1).then(() => h)
  if (o.p !== undefined && d !== undefined && h.p !== undefined) made++
}
console.log(made)

Bun.gc(true)
console.log("after gc")

// a chain that settles is untouched
let q = Promise.resolve(0)
for (let i = 0; i < 1000; i++) {
  q = q.then((x: number) => x + 1)
}
console.log(await q)

// a ring-shaped reaction on a held source runs once it settles
let release = (v: number) => {}
const held = new Promise<number>((res) => {
  release = res
})
const box: any = { n: 0 }
box.p = held.then((v: number) => {
  box.n = v
  return box
})
Bun.gc(true)
release(42)
const back: any = await box.p
console.log(back === box, box.n)

// reactions on one source still run in registration order
const order: number[] = []
let go = (v: number) => {}
const src = new Promise<number>((res) => {
  go = res
})
for (let i = 0; i < 5; i++) {
  src.then(() => {
    order.push(i)
  })
}
go(1)
await src
await null
console.log(order.join(","))
