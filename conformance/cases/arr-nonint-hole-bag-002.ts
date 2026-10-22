// The same hole bag after a named property lands in it: the
// non-integral read finds the key the bag now holds.
const a: number[] = [1, 2, 3]
const far = parseInt(process.argv[2] ?? "5", 10)
a[far] = 8
const b: any = a
b["1.5"] = 42
b["NaN"] = 11
const half = parseFloat("1.5")
console.log(a[half], a[parseFloat("2.5")], a[parseFloat("NaN")])
delete a[1]
console.log(a[1], a[half], a.length)
