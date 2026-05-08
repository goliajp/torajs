// T-26.B (v0.7) — WeakMap basics. Pointer-identity-keyed map
// with auto-eviction on key death. Set / has / delete are O(1)
// average. We test set + has + delete here; get's typed return
// (Nullable<Any>) needs an `as` cast pass that's a follow-up
// once the conformance harness's bun-parity differ on raw ptr
// printing.

class Box {
  v: number;
  constructor(n: number) { this.v = n; }
}

let m: weakmap = new WeakMap()
let k1 = new Box(1)
let k2 = new Box(2)
let k3 = new Box(3)

m.set(k1, 'alpha')
m.set(k2, 'beta')

console.log(m.has(k1))
console.log(m.has(k2))
console.log(m.has(k3))

console.log(m.delete(k1))
console.log(m.has(k1))
console.log(m.delete(k1))

// Set on the same key replaces the value, doesn't add a duplicate.
m.set(k2, 'beta-replaced')
console.log(m.has(k2))
console.log(m.delete(k2))
console.log(m.has(k2))

// typeof a WeakMap is "object" per spec.
console.log(typeof m)
