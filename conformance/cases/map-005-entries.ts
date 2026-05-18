// P6.4c — Map.entries() yields `[key, value]` Array<Any> pairs per
// spec §23.1.3.4. The runtime allocates a fresh 2-element array each
// step; the IteratorResult struct's `.value` field carries an Any-
// boxed Array<Any>. Verifying the [k, v] decompose requires either
// `arr[i]` on Any-typed receivers (P0 substrate gap — tora can't
// index Any at typed-tier today) or for-of destructuring (P6.4c-C3
// follow-up). This fixture asserts the iter mechanics + done flag
// + insertion order count; deeper `[k, v]` content checks land
// in the C3 for-of fixture.

let m: Map = new Map()
m.set('a', 1)
m.set('b', 2)
m.set('c', 3)

console.log(m.size)

let it = m.entries()
let s1 = it.next()
console.log(s1.done)
console.log(typeof s1.value)

let s2 = it.next()
console.log(s2.done)
console.log(typeof s2.value)

let s3 = it.next()
console.log(s3.done)
console.log(typeof s3.value)

let s4 = it.next()
console.log(s4.done)
console.log(typeof s4.value)
