// V3-18 m3.b — `===` / `!==` cross-type per spec §7.2.15:
// different types → false unconditionally (no throw). Restored
// from the strict same-type-only behavior. Used pervasively in
// test262 for deliberate "this is the wrong type" assertions.
console.log(1 === 1)
console.log(1 === true)
console.log(true === 1)
console.log("1" === 1)
console.log(null === 0)
console.log(null === null)
console.log(true === true)
console.log("a" === "a")
console.log(1 !== true)
console.log("x" !== 5)
