// A Proxy owns its target and its handler. A handler that refers back
// to the proxy it serves is a two-cell ring, and the collector had no
// arm for the tag at all — a Proxy was neither a root nor a place the
// walk could pass through, so the whole ring stayed.

let ok = 0
for (let i = 0; i < 200; i++) {
  // handler -> proxy
  const handler: any = { get(t: any, k: string) { return (t as any)[k] } }
  const p: any = new Proxy({ v: i }, handler)
  handler.owner = p
  if (p.v === i && handler.owner.v === i) ok++

  // target -> proxy
  const target: any = { v: i }
  const q: any = new Proxy(target, {})
  target.self = q
  if (q.self.v === i) ok++

  // two proxies, each other's handler-side reference
  const ha: any = {}
  const hb: any = {}
  const a: any = new Proxy({ n: "a" }, ha)
  const b: any = new Proxy({ n: "b" }, hb)
  ha.peer = b
  hb.peer = a
  if (a.n === "a" && b.n === "b") ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A held proxy ring answers through its traps afterwards.
const h: any = { get(t: any, k: string) { return k === "tag" ? "trapped" : (t as any)[k] } }
const live: any = new Proxy({ v: 7 }, h)
h.owner = live
Bun.gc(true)
console.log(live.v, live.tag, h.owner === live)
