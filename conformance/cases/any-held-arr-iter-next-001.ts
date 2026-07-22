// arr_iter_method dispatch — any-held ArrIter `.next()` returning
// IteratorResult `{value, done}` (ES §23.1.5). Mirrors MapIter's
// method_call_cell arm shipped earlier. `xs.values()` / `.keys()` /
// `.entries()` on any[] holds an ArrIter cell whose direct `.next()`
// used to fall through to "value is not a function on this any
// receiver" — now dispatches to arr_iter_method via method_call_cell.
const vs: any[] = [10, 20, 30]
const vi: any = vs.values()
console.log(vi.next().value, vi.next().value, vi.next().value, vi.next().done)

const ks: any[] = ['a', 'b']
const ki: any = ks.keys()
console.log(ki.next().value, ki.next().value, ki.next().done)

const es: any[] = ['x', 'y']
const ei: any = es.entries()
const r1 = ei.next()
const r2 = ei.next()
const r3 = ei.next()
console.log(JSON.stringify(r1.value), r1.done)
console.log(JSON.stringify(r2.value), r2.done)
console.log(r3.value, r3.done)
