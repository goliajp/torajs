// P6.4b — Set.keys() / Set.values(). Per spec §24.2.3.5 /
// §24.2.3.10 the two are aliases (both yield the elements of the
// set). The MapIter substrate from Map.keys is reused — Set
// stores its elements as Map keys, so iterating "keys" gives us
// the elements directly.

let s: Set = new Set()
s.add('alpha')
s.add('beta')
s.add('gamma')

console.log(s.size)

let it = s.values()
let step1 = it.next()
console.log(step1.done)
console.log(typeof step1.value)
let step2 = it.next()
console.log(step2.done)
console.log(typeof step2.value)
let step3 = it.next()
console.log(step3.done)
console.log(typeof step3.value)
let step4 = it.next()
console.log(step4.done)

// .keys() should be identical to .values() per spec.
let it2 = s.keys()
let k1 = it2.next()
console.log(k1.done)
console.log(typeof k1.value)
let k2 = it2.next()
console.log(k2.done)
console.log(typeof k2.value)
