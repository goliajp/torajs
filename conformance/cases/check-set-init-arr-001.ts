// `new Set(iterable)` — ES §24.2.1 with typed Array<T> initializer.
// Literal-array path (S133-4) was already covered; this fixture
// exercises the runtime Array<T> path (spread + literal mix,
// function-returned array, Set-from-Set via spread).

const a = new Set([1, 2, 3, 2])
console.log(a.size)

// case B: spread + literal in array literal
const b = new Set([...a, 100])
console.log(b.size)
const sum_b: any[] = []
for (const x of b) sum_b.push(x)
console.log(sum_b)

// case C: spread another Set into new Set
const c = new Set([...a])
console.log(c.size)

// case D: function returns typed array
function makeArr(): number[] {
  return [10, 20, 30, 10]
}
const d = new Set(makeArr())
console.log(d.size)

// case E: Array<string>
const e = new Set(["a", "b", "c", "a"])
console.log(e.size)
const arr_e: any[] = []
for (const x of e) arr_e.push(x)
console.log(arr_e)

// case F: spread of Array<string>
const src_f: string[] = ["x", "y", "x", "z"]
const f = new Set([...src_f])
console.log(f.size)
