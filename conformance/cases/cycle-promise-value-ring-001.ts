// A settled Promise owns its value — the drop kernel's
// `value_drop_heap((*pp).value)` says so plainly. Rotation 528 gave
// the tag its property bag, and that bag was the only child the walk
// enumerated, so `const p = Promise.resolve(o); o.p = p` was a
// two-cell ring nothing could reach.
//
// Not covered, deliberately: a ring running through a pending
// callback. Each callback record packs the emit side's opaque `arg`
// word, which the runtime cannot read and the drop kernel does not
// follow either.
//
// Observable here: the walk crosses these rings without crashing and
// a live one still settles to the same object. Whether the dead ones
// are reclaimed is a peak-RSS question, measured out of band.

let ok = 0
for (let i = 0; i < 200; i++) {
  // value edge: promise -> object -> promise
  const o: any = { v: i }
  const p: any = Promise.resolve(o)
  o.p = p
  if (o.p === p) ok++

  // two promises, each settled with a holder of the other
  const ha: any = {}
  const hb: any = {}
  const pa: any = Promise.resolve(ha)
  const pb: any = Promise.resolve(hb)
  ha.peer = pb
  hb.peer = pa
  if (ha.peer === pb && hb.peer === pa) ok++

  // a rejected promise owns its reason the same way
  const reason: any = { code: i }
  const pr: any = Promise.reject(reason)
  reason.self = pr
  pr.catch(() => {})
  if (reason.self === pr) ok++

  // through an array the promise settled with
  const arr: any[] = [i]
  const pv: any = Promise.resolve(arr)
  arr.push(pv)
  if (arr[1] === pv) ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A held ring still settles to the same object it points back from.
const live: any = { n: 7 }
const lp: any = Promise.resolve(live)
live.p = lp
Bun.gc(true)
const back: any = await lp
console.log(back.n, back === live, back.p === lp)
