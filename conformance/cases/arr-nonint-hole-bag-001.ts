// A typed number[] whose props bag holds only hole entries (the gap
// of a grow past the end) answers a non-integral index from its own
// slots' undefined: no named property can be in the bag.
const a: number[] = [1, 2, 3]
const far = parseInt("6", 10) | 0
a[far] = 7
const half = parseFloat("1.5")
const v = a[half]
const w = a[parseFloat("NaN")]
console.log(v === undefined ? "absent" : "present")
console.log(w === undefined ? "absent" : "present")
console.log(a.length)
a[far - 2] = 9
console.log(a[4] + a[6])
const x = a[half + 3]
console.log(x === undefined ? "absent" : "present")
