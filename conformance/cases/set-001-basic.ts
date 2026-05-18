// P6.2 — Set<T> basics. Backed by the P6.1 Map runtime with the
// value side pinned to ANY_UNDEF; SameValueZero element equality
// flows from the same key-comparison logic the Map uses. We test
// add + has + delete + clear + size against three element flavors
// (string, number, object-identity) plus the dedup semantics
// (`s.add(x); s.add(x)` keeps size at 1).

class Box {
  v: number
  constructor(n: number) {
    this.v = n
  }
}

let s: Set = new Set()

let b1 = new Box(1)
let b2 = new Box(2)
let b3 = new Box(3)

s.add('alpha')
s.add('beta')
s.add(42)
s.add(b1)
s.add(b2)

console.log(s.size)
console.log(s.has('alpha'))
console.log(s.has('gamma'))
console.log(s.has(42))
console.log(s.has(43))
console.log(s.has(b1))
console.log(s.has(b3))

// SameValueZero dedup — adding an existing element is a no-op,
// size stays put.
s.add('alpha')
s.add(42)
console.log(s.size)

// Delete returns true on first removal, false on the second.
console.log(s.delete('beta'))
console.log(s.delete('beta'))
console.log(s.has('beta'))
console.log(s.size)

console.log(s.delete(b1))
console.log(s.has(b1))
console.log(s.has(b2))

// Clear wipes everything; size goes to 0.
s.clear()
console.log(s.size)
console.log(s.has('alpha'))
console.log(s.has(42))
console.log(s.has(b2))

// typeof a Set is "object" per spec.
console.log(typeof s)
