// ES2023 §23.1.3.42 — `xs.toSpliced(start, deleteCount)` immutable
// splice. Returns a fresh `Array<T>` with the [start, start+deleteCount)
// range removed; source untouched (unlike `xs.splice` which mutates
// in-place + returns the removed sub-array). Subset matches splice's
// fixed 2-arg shape; the variadic `...items` insert form is a follow-up.

// number[] — middle removal
const ns: number[] = [10, 20, 30, 40, 50]
console.log('ns toSpliced(1, 2)', ns.toSpliced(1, 2))
console.log('ns after', ns)

// number[] — head removal
console.log('ns toSpliced(0, 2)', ns.toSpliced(0, 2))
console.log('ns after head removal', ns)

// number[] — tail removal
console.log('ns toSpliced(3, 99)', ns.toSpliced(3, 99))
console.log('ns after tail removal', ns)

// number[] — zero delete (identity copy)
console.log('ns toSpliced(2, 0)', ns.toSpliced(2, 0))
console.log('ns after zero delete', ns)

// string[] — refcounted-elem path exercises rc_inc / drop walk
const ss: string[] = ['alpha', 'bravo', 'charlie', 'delta', 'echo']
console.log('ss toSpliced(1, 2)', ss.toSpliced(1, 2))
console.log('ss after', ss)

// boolean[]
const bs: boolean[] = [true, false, true, true, false]
console.log('bs toSpliced(1, 3)', bs.toSpliced(1, 3))

// empty arr
const e: number[] = []
console.log('empty toSpliced(0, 0)', e.toSpliced(0, 0))
console.log('empty toSpliced(5, 5)', e.toSpliced(5, 5))

// independence — mutating the result doesn't affect source
const orig: number[] = [1, 2, 3, 4, 5]
const out = orig.toSpliced(1, 2)
out[0] = 999
console.log('orig[0]', orig[0])
console.log('out[0]', out[0])
