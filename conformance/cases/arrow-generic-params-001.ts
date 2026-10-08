// a generic arrow runs as the same arrow typed per call
const id = <T>(x: T): T => x
console.log(id(3), id("s"))
const pair = <A, B>(a: A, b: B) => [a, b]
console.log(pair(1, "x").length)
const keyOf = <T extends object>(o: T) => Object.keys(o)
console.log(keyOf({ q: 1, r: 2 }).join())
const trail = <T,>(x: T, n: number) => [x, n]
console.log(trail(true, 2).join())
const withDefault = <T = string>(x: T) => typeof x
console.log(withDefault(5), withDefault("a"))
const nums = [3, 1, 2]
const firstOr = <T>(xs: T[], d: T): T => (xs.length > 0 ? xs[0] : d)
console.log(firstOr(nums, 0), firstOr<string>([], "none"))
const n = <number>(5 as any)
console.log(n + 1)
