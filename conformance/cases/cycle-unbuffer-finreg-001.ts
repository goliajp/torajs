// §26.2 — a chain of FinalizationRegistry cells, each holding the
// previous registry as the held value of a live target. Releasing the
// head releases every level; the bottom registry has a pending cleanup
// (its target died first), so it outlives the chain and its callback
// still runs. A registry is scrubbed from the cycle-root buffer before
// its held values go, which must not change which callbacks run.
// tr reclaims deterministically, so callbacks run at the program's exit
// checkpoint, after microtasks; bun (a tracing collector) runs none of
// them without a forced gc, which the spec equally allows. The expected
// output is tr's.
const keep = {}
let bottom: FinalizationRegistry<string> | null = new FinalizationRegistry((held: string) => {
  console.log("bottom cleanup", held)
})
;(function () {
  bottom!.register({}, "target died before the chain went")
})()
bottom.register(keep, "target kept to the end")

let head: any = bottom
bottom = null
for (let i = 0; i < 50000; i++) {
  const r = new FinalizationRegistry((held: any) => {
    console.log("level cleanup", i, typeof held)
  })
  r.register(keep, head)
  head = r
}
console.log("chain built")
head = null
console.log("chain released")

// a registry holding a Map chain and a proxy chain as held values,
// released while those targets are alive
const holder = new FinalizationRegistry((held: any) => {
  console.log("holder cleanup", typeof held)
})
let mm: any = new Map()
for (let i = 0; i < 2000; i++) {
  const x = new Map<string, any>()
  x.set("prev", mm)
  mm = x
}
let pp: any = {}
for (let i = 0; i < 2000; i++) pp = new Proxy(pp, {})
holder.register(keep, mm)
holder.register(keep, pp)
mm = null
pp = null
;(function () {
  holder.register({}, "dies now")
})()

Promise.resolve().then(() => console.log("microtask"))
console.log("sync end")
