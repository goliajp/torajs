// S224 — String.{startsWith, endsWith, includes}(needle, undefined)
// per ES §22.1.3.{21,5} (startsWith/includes: position=undef →
// ToIntegerOrInfinity(undefined)=0) and §22.1.3.7 (endsWith:
// endPosition=undef → length(O)).

const s = "abcdef"

console.log("=== startsWith ===")
console.log(s.startsWith("ab", undefined))
console.log(s.startsWith("cd", undefined))
console.log(s.startsWith("", undefined))

console.log("=== endsWith ===")
console.log(s.endsWith("ef", undefined))
console.log(s.endsWith("ab", undefined))
console.log(s.endsWith("", undefined))

console.log("=== includes ===")
console.log(s.includes("cd", undefined))
console.log(s.includes("xx", undefined))
console.log(s.includes("", undefined))

// non-undef regression — numeric position arg still works.
console.log("=== non-undef ===")
console.log(s.startsWith("cd", 2))
console.log(s.endsWith("cd", 4))
console.log(s.includes("ef", 3))

// empty receiver edge cases.
console.log("=== empty ===")
console.log("".startsWith("", undefined))
console.log("".endsWith("", undefined))
console.log("".includes("", undefined))
