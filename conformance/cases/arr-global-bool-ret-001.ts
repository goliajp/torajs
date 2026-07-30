// An Any-boxed value crossing a declared `boolean` return boundary
// coerces (ToBoolean) instead of flowing the box raw into the Bool
// slot — the raw box read back as whatever the caller's zero-test
// happened to check. The promoted bool index read (boolean[]
// elements box so OOB can spell undefined) is the live producer of
// that shape.
const flags: boolean[] = [true, false];
function first(): boolean { return flags[0] }
function second(): boolean { return flags[1] }
console.log(first())
console.log(second())
console.log(flags[0], flags[1])
