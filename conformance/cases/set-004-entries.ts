// P6.4c — Set.entries() yields `[value, value]` Array<Any> pairs per
// spec §24.2.3.6 (the storage's value side is always ANY_UNDEF, so
// iteration exposes the element twice — both callback args receive
// the same value). Verifies iter mechanics + done flag. The deeper
// `[v, v]` decompose verify lands in the C3 for-of fixture (same
// P0 Any.Index gap as map-005).

let s: Set = new Set()
s.add('a')
s.add('b')
s.add('c')

console.log(s.size)

let it = s.entries()
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
