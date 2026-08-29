// A Map / Set / Date / RegExp / Promise / buffer / iterator whose own
// -property bag points back at itself (`m.self = m`) is a whole cycle
// with a single member. Rotation 528 taught the collector to walk
// those shapes, but their drop kernels returned the moment the
// refcount stayed positive, without registering a candidate — so such
// a ring never entered the root buffer at all. Only a ring reached
// through a parent that was itself a root (a named class instance, an
// array, a closure) could be collected.
//
// What user code can observe is that the collection runs over these
// rings and that a live one is untouched; the freeing itself is
// measured out of band (peak RSS on the AOT product).

function selfRing(cell: any): boolean {
  cell.self = cell
  return cell.self === cell
}

let ok = 0
for (let i = 0; i < 200; i++) {
  if (selfRing(new Map([["k", i]]))) ok++
  if (selfRing(new Set([i]))) ok++
  if (selfRing(new Date(i))) ok++
  if (selfRing(/x/g)) ok++
  if (selfRing(Promise.resolve(i))) ok++
  if (selfRing(new ArrayBuffer(8))) ok++
  if (selfRing(new Uint8Array(2))) ok++
  if (selfRing(new DataView(new ArrayBuffer(8)))) ok++
  if (selfRing([i].values())) ok++
  if (selfRing([i].values().map((x: number) => x))) ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A self-ring that is still held survives the collection whole.
const live = new Map([["k", 7]])
selfRing(live)
Bun.gc(true)
const back: any = (live as any).self
console.log(live.get("k"), back === live, back.get("k"))
