// `Object(Symbol())` mints a SymbolWrapper. It carries an expando bag
// at the same offset its three sibling wrappers do, and the drop
// dispatcher has always offered it to the cycle root buffer — but
// `is_visitable_wrapper` named only Number, String and Boolean, so
// the offer was refused and `w.self = w` stayed forever. The corpse
// route had the same hole: a SymbolWrapper reaching the deferred
// free would have been read at the array spill offset and freed
// without releasing its inner Symbol.

const sym = Symbol("s")
let ok = 0
for (let i = 0; i < 200; i++) {
  const w: any = Object(Symbol("k" + i))
  w.self = w
  w.n = i
  if (w.self === w && w.n === i) ok++

  // through a sibling wrapper, so the ring spans two shapes
  const s: any = Object("str" + i)
  const b: any = Object(true)
  s.b = b
  b.s = s
  if (s.b.s === s) ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A held wrapper ring keeps its expando and its wrapped primitive.
const live: any = Object(sym)
live.self = live
Bun.gc(true)
console.log(live.self === live, typeof live.valueOf(), live.valueOf() === sym)
