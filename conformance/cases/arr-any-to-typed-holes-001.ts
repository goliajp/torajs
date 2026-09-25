// An any array with holes bound to a typed annotation: the holes
// carry over as holes (not own properties, read undefined), not a
// TypeError for an element that does not exist.
const s: string[] = ["p", , "q"] as any
console.log(s.length, 1 in s, s[1], s.join("|"), Object.keys(s).join(","))
const n: number[] = [1.5, , 3.5] as any
console.log(n.length, 1 in n, n[1], typeof n[1], n.indexOf(3.5))
let seen = 0
n.forEach(() => seen++)
console.log(seen)
const b: boolean[] = [true, , false] as any
console.log(b.length, 1 in b, b[1])
const o: { v: number }[] = [{ v: 1 }, , { v: 3 }] as any
console.log(o.length, 1 in o, o[1], o.map((x) => x.v).join("|"))
