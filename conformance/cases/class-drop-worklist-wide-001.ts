// A class whose instances hold an array of their own class, released
// through the worklist: wide nodes (more children than the pending
// stack holds), a chain 40k deep through one- and two-element arrays,
// shared children, a child kept alive past its tree, and WeakRefs into
// the tree. Every shape is built and dropped twice.
class Wide {
  v: number
  kids: Wide[] = []
  constructor(v: number) { this.v = v }
}
function wide(levels: number, fan: number, v: number): Wide {
  const w = new Wide(v)
  if (levels > 0) {
    for (let i = 0; i < fan; i++) w.kids.push(wide(levels - 1, fan, v * fan + i))
  }
  return w
}
function chain(n: number): Wide {
  let w = new Wide(0)
  for (let i = 1; i < n; i++) {
    const p = new Wide(i)
    p.kids.push(w)
    if (i % 5 === 0) p.kids.push(new Wide(-i))
    w = p
  }
  return w
}
function count(w: Wide): number {
  let n = 0
  const work: Wide[] = [w]
  while (work.length > 0) {
    const x = work.pop()!
    n++
    for (let i = 0; i < x.kids.length; i++) work.push(x.kids[i])
  }
  return n
}

// each tree dies when its function returns; the WeakRefs into it
// outlive it and may or may not still see their nodes
const refs: WeakRef<Wide>[] = []
const vals: number[] = []
function wideRound(): Wide {
  const a = wide(2, 300, 1)
  const kept = a.kids[7].kids[11]
  for (const w of [a, a.kids[0], a.kids[299].kids[299], kept]) {
    refs.push(new WeakRef(w))
    vals.push(w.v)
  }
  console.log("wide", count(a), kept.v)
  return kept
}
function chainRound(): void {
  const c = chain(40000)
  console.log("chain", count(c), c.v)
}
function sharedRound(shared: Wide): void {
  const s = new Wide(5)
  for (let i = 0; i < 100; i++) s.kids.push(i % 2 === 0 ? shared : new Wide(i))
  console.log("shared", count(s), count(shared))
}

for (let round = 0; round < 2; round++) {
  const kept = wideRound()
  const ref = new WeakRef(kept)
  console.log("kept", kept.v, count(kept), ref.deref() === kept)
  chainRound()
  const shared = wide(1, 40, 9)
  sharedRound(shared)
  console.log("after", count(shared), shared.kids[39].v)
  let ok = true
  for (let i = 0; i < refs.length; i++) {
    const d = refs[i].deref()
    if (d !== undefined && d.v !== vals[i]) ok = false
  }
  console.log("weak", ok, refs.length)
}
