// A STRING receiver's key domain is the whole value domain. §7.1.19
// spells every value into a property key FIRST; only then does
// §10.4.3 ask whether the spelling is a canonical index naming a
// character. So the coerced keys are told apart from the element
// lane by their SPELLING, at runtime, by the same keyed kernel that
// has always answered the `any` spelling of these programs.
const s = "abc"

// canonical index spellings reach the character lane, whatever the
// key's static type
const bi = 1n
console.log(s[bi])
console.log(s[[1]])
const ok = { toString() { return "2" } }
console.log(s[ok])
let kd = "1"
kd += ""
console.log(s[kd])
const ak: any = "1"
console.log(s[ak])

// every other spelling is an ordinary property read, and a miss
console.log(s[true])
console.log(s[false])
console.log(s[null])
console.log(s[undefined])
console.log(s[{}])
console.log(s[/ab/])
console.log(s[new Map()])
console.log(s[new Set()])
console.log(s[Symbol("k")])
console.log(s[function () {}])
class Box {
  v = 1
}
console.log(s[new Box()])
console.log(s[[9]])

// the number lane is untouched, out of range included
console.log(s[0], s[2], s[5], s[-1])

// a SUBSTR receiver rides the same lane
const sub = s.slice(1)
console.log(sub[0], sub[bi], sub[true], sub[undefined], sub[[0]])
