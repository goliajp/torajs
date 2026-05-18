// P6.1 — Map<K,V> basics. Open-addressing robin-hood hash table
// with SameValueZero key equality. We exercise set + has + delete +
// size + clear here against three key flavors (string, number,
// object-identity). The get path returns Nullable<Any> in the
// typechecker; printing the unboxed value requires an `as` cast
// pass that's a P6 follow-up, so this fixture stays on the boolean /
// number-returning slice.

class Box {
  v: number
  constructor(n: number) {
    this.v = n
  }
}

let m: Map = new Map()

let k1 = new Box(1)
let k2 = new Box(2)
let k3 = new Box(3)

// Mixed key types — string + number + heap identity all map to
// distinct slots via the (tag, payload) hash domain.
m.set('alpha', 100)
m.set('beta', 200)
m.set(42, 'forty-two')
m.set(k1, 'box-one')
m.set(k2, 'box-two')

console.log(m.size)
console.log(m.has('alpha'))
console.log(m.has('beta'))
console.log(m.has('gamma'))
console.log(m.has(42))
console.log(m.has(43))
console.log(m.has(k1))
console.log(m.has(k2))
console.log(m.has(k3))

// Set on an existing key replaces value, doesn't add a duplicate.
m.set('alpha', 101)
console.log(m.size)
console.log(m.has('alpha'))

// Delete returns true on first removal, false on the second.
console.log(m.delete('beta'))
console.log(m.delete('beta'))
console.log(m.has('beta'))
console.log(m.size)

// Object-identity keys: k1 is the live ref; deleting it leaves k2.
console.log(m.delete(k1))
console.log(m.has(k1))
console.log(m.has(k2))

// Clear wipes the table; size goes to 0 and previously-set keys
// drop out.
m.clear()
console.log(m.size)
console.log(m.has('alpha'))
console.log(m.has(42))
console.log(m.has(k2))

// typeof a Map is "object" per spec §13.5.3.
console.log(typeof m)
