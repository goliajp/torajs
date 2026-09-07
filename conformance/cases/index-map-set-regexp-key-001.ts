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
