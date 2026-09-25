// for-of over a typed array after an element delete: the iterator's
// Get of the hole answers undefined (§23.1.5.1 %ArrayIteratorPrototype%
// .next reads O[index]), not the slot's raw sentinel.
const a: number[] = [1.5, 2.5, 3.5]
delete a[1]
for (const v of a) console.log(String(v), typeof v, v === undefined)
let n = 0
for (const v of a) if (v !== undefined) n += v
console.log(n)
const s: string[] = ["x", "y", "z"]
delete s[0]
for (const v of s) console.log(String(v), typeof v, v === undefined)
const [p, q] = a
console.log(p, q, typeof q)
