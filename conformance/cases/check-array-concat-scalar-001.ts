// ES §23.1.3.2 — Array.prototype.concat accepts a mix of Array<T> and
// bare T values. Array slots are spread; scalar slots are appended as
// single elements. tora previously required every arg to be Array<T>,
// rejecting `xs.concat([4,5], 6, [7,8])` with arity check noise.
const a = [1, 2, 3]
// scalar in middle
console.log(a.concat([4, 5], 6, [7, 8]))
// scalar leading
console.log(a.concat(0, [4]))
// scalar trailing
console.log(a.concat([10], 20))
// all scalar
console.log(a.concat(100, 200))
// receiver unchanged (concat returns fresh)
console.log(a)
// string elem
const s: string[] = ['x', 'y']
console.log(s.concat('z', ['w']))
