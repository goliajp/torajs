// A typed array's non-integral read reaches a named property whose
// value has to be coerced or computed: an object's valueOf, an
// accessor's getter, and a string-typed array's object value.
const xs: number[] = [1, 2]
Object.defineProperty(xs, "1.5", { value: { valueOf() { return 7 } } })
console.log(xs[parseFloat("1.5")] + 0)
const ys: number[] = [1]
Object.defineProperty(ys, "0.5", { get() { return 3 } })
console.log(ys[parseFloat("0.5")])
const zs: number[] = [4, 5]
console.log(zs[parseFloat("0.5")], zs[parseFloat("1")])
