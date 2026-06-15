// S127-1 — Array<Any>.indexOf/lastIndexOf/includes with `undefined`
// needle must pack as ANY_UNDEF (tag=5), not ANY_NULL (tag=0).
// Pre-fix, `undefined` lowered to ConstPtrNull (same shape as `null`)
// and the strict_eq needle packing arm collapsed both to tag=0, so
// `xs.indexOf(undefined)` wrongly matched the first null slot.
const xs: any[] = [1, "two", true, null, undefined, 5]
console.log("indexOf null =", xs.indexOf(null))
console.log("indexOf undef =", xs.indexOf(undefined))
console.log("lastIndexOf null =", xs.lastIndexOf(null))
console.log("lastIndexOf undef =", xs.lastIndexOf(undefined))
console.log("includes null =", xs.includes(null))
console.log("includes undef =", xs.includes(undefined))
// 99 not in xs — both undef and null sentinel must NOT match it.
console.log("indexOf 99 =", xs.indexOf(99))
console.log("includes 99 =", xs.includes(99))
// undef-only arr — null search must miss.
const ys: any[] = [undefined, undefined]
console.log("ys.indexOf null =", ys.indexOf(null))
console.log("ys.indexOf undef =", ys.indexOf(undefined))
console.log("ys.includes null =", ys.includes(null))
console.log("ys.includes undef =", ys.includes(undefined))
