// The cycle collector used to descend into class instances, arrays,
// dynobj dicts, closures and primitive wrappers only. A Map / Set /
// Date / RegExp / Promise / buffer / iterator holding a back
// reference in its own-property bag was a wall: the walk stopped
// there and the cycle behind it stayed. This runs the shapes through
// a manual collection — what user code can observe is that the walk
// and the corpse teardown do not crash and the live values survive.
class Holder {
  tag: string
  m: any
  constructor(tag: string) {
    this.tag = tag
    this.m = null
  }
}

function ring(tag: string, cell: any): string {
  const h = new Holder(tag)
  h.m = cell
  cell.back = h
  return h.tag + ":" + (cell.back.m === cell)
}

const made: string[] = []
for (let i = 0; i < 40; i++) {
  made.push(ring("map", new Map([["k", i]])))
  made.push(ring("set", new Set([i])))
  made.push(ring("date", new Date(i)))
  made.push(ring("regexp", /x/g))
  made.push(ring("promise", Promise.resolve(i)))
  made.push(ring("buffer", new ArrayBuffer(8)))
  made.push(ring("typed", new Uint8Array(2)))
  made.push(ring("view", new DataView(new ArrayBuffer(8))))
  made.push(ring("iter", [i].values()))
  made.push(ring("helper", [i].values().map((x: number) => x)))
}
console.log(made.length, made[0], made[9], made[389])

Bun.gc(true)
console.log("after gc")

// The live ring is still whole after the collection.
const keep = new Map([["k", 1]])
const kh = new Holder("kept")
kh.m = keep
;(keep as any).back = kh
Bun.gc(true)
const back: any = (keep as any).back
console.log(back.tag, keep.get("k"), back.m === keep)
