// A copy of an array with holes (Array.from, spread, slice, concat,
// map, the copying methods) holds undefined where the source had a
// hole; a callback over the copy sees undefined, not a raw slot
const a: number[] = [1, 2, 3]
delete a[1]
Array.from(a).forEach((x) => console.log("from", typeof x, x))
const sp = [...a]
sp.forEach((x) => console.log("spread", typeof x))
const sl = a.slice(0)
console.log("slice", sl.map((x) => typeof x).join(","), 1 in sl)
const cc = [0].concat(a)
cc.forEach((x) => console.log("concat", typeof x))
const rv = a.toReversed()
rv.forEach((x) => console.log("reversed", typeof x))
const twice = Array.from(Array.from(a))
twice.forEach((x) => console.log("twice", typeof x))
