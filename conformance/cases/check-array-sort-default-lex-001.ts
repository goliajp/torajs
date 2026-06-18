// ES §23.1.3.30 - Array.prototype.sort: when no comparator is provided,
// elements are converted to strings and compared as code-unit sequences
// (so [10, 2] sorts to [10, 2] because "10" < "2"). bun follows spec;
// tr previously emitted a numeric > comparison which gave the wrong order.

const a: number[] = [10, 2, 1, 20, 100, 3]
a.sort()
console.log(a)

const b: number[] = [-1, -10, 0, 5, -2]
b.sort()
console.log(b)

const c: number[] = [10, 2, 1, 20, 100, 3]
const cSorted = c.toSorted()
console.log(cSorted)
console.log(c)

const d: number[] = [3.14, 2.5, 1.0, 20.0, 100.5]
d.sort()
console.log(d)

const e: boolean[] = [true, false, true, false]
e.sort()
console.log(e)

// User comparator still works untouched.
const f: number[] = [10, 2, 1, 20, 100, 3]
f.sort((x, y) => x - y)
console.log(f)
