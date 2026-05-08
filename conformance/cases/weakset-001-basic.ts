// T-26.B (v0.7) — WeakSet basics. Pointer-identity-keyed set
// with auto-eviction on key death.

class Box {
  v: number;
  constructor(n: number) { this.v = n; }
}

let s: weakset = new WeakSet()
let k1 = new Box(1)
let k2 = new Box(2)
let k3 = new Box(3)

s.add(k1)
s.add(k2)

console.log(s.has(k1))
console.log(s.has(k2))
console.log(s.has(k3))

console.log(s.delete(k1))
console.log(s.has(k1))
console.log(s.delete(k1))

// add is idempotent — adding twice doesn't throw or change has.
s.add(k2)
s.add(k2)
console.log(s.has(k2))
console.log(s.delete(k2))
console.log(s.has(k2))

// typeof a WeakSet is "object" per spec.
console.log(typeof s)
