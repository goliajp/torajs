// Class instances whose cell only crosses joins through stack slots
// (the inliner's return slot, a `let` slot) move to the frame; every
// shape that lets the cell out, keeps an old one alive across the
// allocation, or puts two cells in one slot stays on the heap. All
// answers are the same either way.

class Vec2 {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
  norm(): number {
    return this.x * this.x + this.y * this.y
  }
  scale(k: number): Vec2 {
    return new Vec2(this.x * k, this.y * k)
  }
}

// the bench shape
let total = 0
for (let i = 0; i < 1000; i = i + 1) {
  const v = new Vec2(i, i + 1)
  const s = v.scale(2)
  total = total + s.norm()
}
console.log(total)

// conditional construction: the slot holds the cell or null
let acc = 0
for (let i = 0; i < 10; i = i + 1) {
  const v: Vec2 | null = i % 2 === 0 ? new Vec2(i, i) : null
  if (v !== null) acc = acc + v.x
}
console.log(acc)

// kept past the loop: the previous cell is still referenced
let last: Vec2 | null = null
for (let i = 0; i < 5; i = i + 1) {
  last = new Vec2(i, i * 3)
}
console.log(last !== null ? last.y : -1)

// into an array, into a closure
const xs: Vec2[] = []
for (let i = 0; i < 3; i = i + 1) xs.push(new Vec2(i, 1))
const held = new Vec2(7, 8)
const readX = () => held.x
console.log(xs[2].x + xs[0].y, readX())

// out of a function the inliner leaves alone (recursive)
function chain(n: number): Vec2 {
  if (n <= 0) return new Vec2(1, 1)
  const inner = chain(n - 1)
  return new Vec2(inner.x + 1, inner.y * 2)
}
console.log(chain(4).x, chain(4).y)

// frozen: the field write still throws
const f = Object.freeze(new Vec2(5, 6))
try {
  ;(f as any).x = 9
  console.log("wrote", f.x)
} catch (e) {
  console.log("frozen", e instanceof TypeError, f.x)
}

// identity: a cell equals itself and no other
const a = new Vec2(1, 2)
const b = a.scale(2)
console.log(a === a, a === b, b.x)

// a constructor that throws after writing a field
class Bad {
  x: number
  constructor() {
    this.x = 1
    throw new Error("ctor")
  }
}
let caught = 0
for (let i = 0; i < 3; i = i + 1) {
  try {
    new Bad()
  } catch (e) {
    caught = caught + 1
  }
}
console.log(caught)

// two cells through one binding
let w = new Vec2(1, 1)
w = new Vec2(2, 2)
console.log(w.norm())
