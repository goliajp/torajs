// A loop-local object that a WeakRef points at, and a frozen
// loop-local object whose field write must throw (class bodies are
// strict).
class Box {
  v: number
  constructor(v: number) {
    this.v = v
  }
  set(n: number): void {
    this.v = n
  }
}

let seen = 0
const refs: WeakRef<Box>[] = []
for (let i = 0; i < 20; i = i + 1) {
  const b = new Box(i)
  const r = new WeakRef(b)
  const t = r.deref()
  if (t !== undefined && t === b) seen = seen + t.v
  if (i % 5 === 0) refs.push(r)
}
console.log("weak", seen, refs.length)

let threw = 0
let kept = 0
for (let i = 0; i < 10; i = i + 1) {
  const b = new Box(i)
  if (i % 2 === 0) Object.freeze(b)
  try {
    b.set(i + 100)
  } catch (e) {
    if (e instanceof TypeError) threw = threw + 1
  }
  kept = kept + b.v
}
console.log("frozen", threw, kept)
