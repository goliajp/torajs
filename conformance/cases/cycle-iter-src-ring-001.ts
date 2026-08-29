// A stateful iterator holds a strong reference to what it walks, so
// that iteration outlives the caller's binding. Rotation 528 gave
// MapIter and ArrIter their lazy property bag, and that bag was the
// only child the collector ever enumerated for them — the reference
// the cell exists to hold was not an edge the walk could follow, so
// an iterator reachable from its own source kept the pair alive.
//
// Observable here: the walk runs over these rings without crashing
// and a live one still iterates. Whether the dead ones are reclaimed
// is a peak-RSS question, measured out of band.

let ok = 0
for (let i = 0; i < 200; i++) {
  // array -> iterator -> array
  const a: any[] = [1, 2, 3]
  const it: any = a.values()
  a.push(it)
  if (a.length === 4 && a[3] === it) ok++

  // map -> iterator -> map
  const m = new Map<string, any>()
  m.set("k", i)
  const mi: any = m.keys()
  m.set("it", mi)
  if (m.size === 2 && m.get("it") === mi) ok++

  // the source edge and the bag edge at once
  const b: any[] = [i]
  const bi: any = b.values()
  bi.owner = b
  b.push(bi)
  if (bi.owner === b && b[1] === bi) ok++

  // entries() over a Set, same cell shape
  const s = new Set<any>()
  s.add(i)
  const si: any = s.entries()
  s.add(si)
  if (s.size === 2 && s.has(si)) ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A held ring still iterates, and its source is still the array that
// holds it.
const live: any[] = [10, 20]
const liveIt: any = live.values()
live.push(liveIt)
Bun.gc(true)
console.log(live.length, liveIt.next().value, liveIt.next().value, live[2] === liveIt)
