// A Map, a Set and a RegExp are ordinary objects with an ordinary
// property surface. `m[Symbol.iterator]` is a prototype-surface read
// and `m[k] = v` is an expando store — neither has an element lane
// to collide with, which is why the checker has answered `Any` for
// their non-number keys since r289.
//
// The LOWERING never learned them. A Map or Set receiver died
// "index access on non-array type Map" on every key, the number
// included, and the write side had no arm for any of the three at
// all — so a receiver whose read answers a property could not store
// one. Boxing is the whole mechanism: RegExp has been on the read
// lane's box list the whole time and its non-number reads have
// worked the whole time.
//
// A literal string key (`m["a"]`) is a different question and stays
// where it was: it routes through the member checker, which answers
// nominally on a Map / Set / RegExp and rejects an unknown name.
// Registered, not fixed here.
const sk = Symbol("s")
let kd = "a"
kd += ""
const ak: any = "a"

const m = new Map()
console.log(m[sk], m[kd], m[ak], m[undefined], m[0])
m[sk] = 1
m[kd] = 2
m[ak] = 3
m[undefined] = 4
console.log(m[sk], m[kd], m[ak], m[undefined], m.size)

const s = new Set()
s[sk] = 5
s[kd] = 6
console.log(s[sk], s[kd], s[0], s.size)

const re = /ab/
re[sk] = 7
re[kd] = 8
console.log(re[sk], re[kd], re[undefined])

// the namespaces store the same way — `Math[k] = v` is a keyed
// expando write on the singleton dynobj
Math[kd] = 9
Math[sk] = 10
console.log(Math[kd], Math[sk], Math[undefined])

// and the real surfaces are untouched by the expandos
m.set("k", 1)
console.log(m.get("k"), m.size, s.size, re.test("ab"), re.source)
console.log(typeof m[Symbol.iterator], typeof s[Symbol.iterator])

// And their key domain is the whole non-number domain, coerced keys
// included — none of these receivers owns an element lane, so
// §7.1.19's spelling of the key is the only thing that has to
// happen and the keyed kernel does it off the key's runtime tag.
// Measured across the domain rather than on one member, because
// rotation 608 read a one-word attempt at exactly this as the
// domain's fault when what was missing was the lane above.
class K { x = 1 }
const a2 = [1, 2]
const re2 = /zz/
const m2 = new Map()
const g = function () {}
m[true] = 11
m[null] = 12
m[10n] = 13
m[a2] = 14
m[re2] = 15
m[m2] = 16
m[new K()] = 17
m[g] = 18
console.log(m[true], m[null], m[10n], m[a2], m[re2], m[m2], m[g])
s[true] = 21
s[10n] = 22
s[a2] = 23
re[true] = 31
re[null] = 32
Math[true] = 41
Math[null] = 42
console.log(s[true], s[10n], s[a2], re[true], re[null], Math[true], Math[null])

// none of it disturbed the real surfaces
console.log(m.get("k"), m.size, s.size, re.test("ab"), Math.max(1, 2))
