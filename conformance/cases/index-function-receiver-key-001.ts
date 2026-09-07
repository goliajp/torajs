// §20.2 — a function is an ordinary object, so it carries an
// ordinary property surface: every property key reads off it and
// stores onto it. A function has no element lane, so nothing has to
// be told apart from §7.1.19's spelling of the key.
const f = function () {}

// symbol keys — the well-known ones every builtin protocol probes
const sk = Symbol("sk")
f[sk] = 7
console.log(f[sk])
f[Symbol.isConcatSpreadable] = true
console.log(f[Symbol.isConcatSpreadable])
console.log(typeof f[Symbol.hasInstance])
console.log(f[Symbol.hasInstance]({}))

// dynamic string, undefined and any keys — all three were dead
// entries on the admit arm before rotation 609
let k = "x"
k += "y"
f[k] = 1
console.log(f["xy"], f[k])
f[undefined] = 2
console.log(f["undefined"])
const ak: any = "z"
f[ak] = 3
console.log(f["z"])

// the coerced half of the domain: §7.1.19 runs ToPropertyKey over an
// object key, and the keyed kernel does it off the runtime tag
const ok = { toString() { return "ok" } }
f[ok] = 4
console.log(f["ok"], f[ok])

// numeric keys stay the stringified expando probe they always were
f[0] = 11
console.log(f[0], f["0"])

// the rest of the coerced domain, measured rather than assumed
// (rotation 608 handed five OTHER receivers this same domain one
// word each and the data refused it — an array owns an element lane
// that has to be told apart from the property lane at runtime. A
// function owns no element lane, so every one of these is just a
// spelled property.)
class K { x = 1 }
const m = new Map()
const arr = [1, 2]
const re = /ab/
const g = function () {}
f[true] = 21
f[null] = 22
f[10n] = 23
f[arr] = 24
f[re] = 25
f[m] = 26
f[new K()] = 27
f[g] = 28
console.log(f["true"], f["null"], f["10"], f["1,2"])
console.log(f["/ab/"], f["[object Map]"], f["[object Object]"])
console.log(f[true], f[null], f[10n], f[arr], f[re], f[m], f[g])
