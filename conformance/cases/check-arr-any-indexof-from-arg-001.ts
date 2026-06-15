// S127-4 — Array<Any>.{indexOf|lastIndexOf|includes}(needle, fromIndex)
// per spec §22.1.3.{13,16,15}. Pre-fix the 2-arg dispatch in check.rs
// strict-compared needle type to elem type with no Any escape, so
// `Array<Any>.indexOf(2, fromIdx)` rejected with "must match elem type
// Any, got Number" — the 1-arg path already accepted cross-type needle
// via the generic Any param skip; this brings the 2-arg path in line.
const xs: any[] = [1, 2, 3, 2, 1, 2]
console.log("idx 2 from 2 =", xs.indexOf(2, 2))
console.log("idx 2 from 4 =", xs.indexOf(2, 4))
console.log("idx 2 from -2 =", xs.indexOf(2, -2))
console.log("idx 'x' from 0 =", xs.indexOf("x", 0))
console.log("last 2 to 3 =", xs.lastIndexOf(2, 3))
console.log("last 2 to -3 =", xs.lastIndexOf(2, -3))
console.log("inc 2 from 3 =", xs.includes(2, 3))
console.log("inc 99 from 0 =", xs.includes(99, 0))
console.log("inc 'x' from 0 =", xs.includes("x", 0))
// Mixed-type Any array with primitive needles
const ys: any[] = [null, "a", true, 0, "b", null]
console.log("ys idx null from 1 =", ys.indexOf(null, 1))
console.log("ys idx 'a' from 1 =", ys.indexOf("a", 1))
console.log("ys inc true from 0 =", ys.includes(true, 0))
console.log("ys inc true from 3 =", ys.includes(true, 3))
