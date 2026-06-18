// ES §23.1.3.11 — Array.prototype.flat returns a fresh array, leaving
// non-Array elements untouched at the given depth. tora previously
// rejected `[1,2,3].flat()` at typecheck with "requires Array<Array<T>>";
// the spec form is a shallow copy (b !== a) with same element type.
const a: number[] = [1, 2, 3]
const b = a.flat()
console.log(b)
console.log(b.length)
console.log(b === a)
const c: string[] = ['x', 'y', 'z']
const d = c.flat()
console.log(d)
console.log(d === c)
// boolean elem
const e: boolean[] = [true, false]
console.log(e.flat())
// mixed Array<Array<T>> still flattens one level
const f: number[][] = [[1, 2], [3]]
console.log(f.flat())
