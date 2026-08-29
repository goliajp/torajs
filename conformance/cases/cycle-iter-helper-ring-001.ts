// An Iterator Helper owns four AnyValues: the underlying iterator,
// the captured callback, flatMap's current inner iterator, and the
// cached `next` method. Every one of them is an edge, and the walk
// followed none — a helper cell answered the collector with its
// property bag and nothing else.
//
// Observable here: the walk crosses these rings without crashing and
// a live one still steps. Whether the dead ones are reclaimed is a
// peak-RSS question, measured out of band.

let ok = 0
for (let i = 0; i < 200; i++) {
  // underlying edge: array -> helper -> ArrIter -> array
  const a: any[] = [1, 2, 3]
  const h: any = a.values().map((x: any) => x)
  a.push(h)
  if (a.length === 4 && a[3] === h) ok++

  // fn edge: helper -> closure -> box -> helper
  const box: any = {}
  const g: any = [i].values().map((x: any) => (box.self ? x : x))
  box.self = g
  if (box.self === g) ok++

  // a chained helper owns the helper below it
  const c: any = [1, 2].values().filter((x: any) => x > 0).map((x: any) => x * 2)
  const holder: any = { c }
  ;(c as any).holder = holder
  if (holder.c === c) ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A held ring still steps, and its callback still fires.
const src: any[] = [5, 6]
const live: any = src.values().map((x: any) => x * 10)
src.push(live)
Bun.gc(true)
console.log(src.length, live.next().value, live.next().value)
