// §7.1.19 spells a key FIRST and only then asks whether the spelling
// is a canonical array index. So a boolean, a null or an object key
// on an array is an ordinary property: it stores under its spelling,
// it never reaches the element lane, and it never moves `length`.
//
// t262 property-cast-boolean-primitive / S15.4_A1.1_T5 / the eight
// Array.prototype higher-order cases that seed `arr[true] = 11`.
const x = []
x[true] = 1
console.log(x[1], x["true"], x[true], x.length)
x[false] = 0
console.log(x[0], x["false"], x[false], x.length)
x[null] = 2
console.log(x["null"], x[null], x.length)

// the coerced half: ToPropertyKey runs OrdinaryToPrimitive, so the
// key is whatever the object's own toString answers
const ok = { toString() { return "kk" } }
x[ok] = 3
console.log(x["kk"], x[ok], x.length)

// the rest of the domain, measured rather than assumed
class K { x = 1 }
const arr2 = [7, 8]
const re = /ab/
const m = new Map()
const g = function () {}
x[10n] = 21
x[arr2] = 22
x[re] = 23
x[m] = 24
x[new K()] = 25
x[g] = 26
console.log(x["10"], x["7,8"], x["/ab/"], x["[object Map]"], x["[object Object]"])
console.log(x[10n], x[arr2], x[re], x[m], x[g], x.length)

// and the element lane is exactly where it was: a canonical index
// spelling still stores an element and still moves length
const y = [1, 2]
y[2] = 3
y["3"] = 4
console.log(y.length, y[2], y[3], y.join(","))
y[true] = 9
console.log(y.length, y[1], y["true"])
