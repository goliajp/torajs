// Array.from(iterable, mapFn) — ES §23.1.2.1.

// case A: typed Array<number> with 1-arg mapFn
const a = Array.from([1, 2, 3], (x: number) => x * 2)
console.log(a)

// case B: typed Array<number> with 2-arg mapFn (idx)
const b = Array.from([10, 20, 30], (v: number, i: number) => v + i)
console.log(b)

// case C: string with 1-arg mapFn yielding string
const c = Array.from("abc", (ch: string) => ch + "!")
console.log(c)

// case D: string with 2-arg mapFn — char + idx
const d = Array.from("xyz", (ch: string, i: number) => ch + i)
console.log(d)

// case E: 1-arg form still works (regression check)
const e = Array.from([7, 8, 9])
console.log(e)

// case F: string → Array<string> 1-arg form still works
const f = Array.from("pq")
console.log(f)

// case G: typed Array<string> → mapped Array<string>
const g = Array.from(["a", "b"], (s: string) => s + s)
console.log(g)

// case H: typed Array<number> → mapped to number (negation)
const h = Array.from([5, -3, 7], (n: number) => -n)
console.log(h)
