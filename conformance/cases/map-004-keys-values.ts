// P6.4b — Map.keys() / Map.values() returning a stateful MapIter.
// `iter.next()` yields `IteratorResult<any>` = `{ value, done }`.
// We drive the iter manually here (P6.4c will wire for-of so user
// code can `for (let k of m.keys())` directly).

let m: Map = new Map()
m.set('alpha', 1)
m.set('beta', 2)
m.set('gamma', 3)

console.log(m.size)

// Drive the keys iterator until done. Each step prints typeof key
// (string for these inputs) so we can observe insertion order.
let it1: mapiter = m.keys()
let step1 = it1.next()
console.log(step1.done)
console.log(typeof step1.value)
let step2 = it1.next()
console.log(step2.done)
console.log(typeof step2.value)
let step3 = it1.next()
console.log(step3.done)
console.log(typeof step3.value)
let step4 = it1.next()
console.log(step4.done)

// Values iter — typeof should be 'number' for our integer values.
let it2: mapiter = m.values()
let s1 = it2.next()
console.log(s1.done)
console.log(typeof s1.value)
let s2 = it2.next()
console.log(s2.done)
console.log(typeof s2.value)
let s3 = it2.next()
console.log(s3.done)
console.log(typeof s3.value)
let s4 = it2.next()
console.log(s4.done)
