// A typed generator's `undefined` completion: an exhausted
// Generator<number> over integer yields reads undefined (its value slot
// cannot be an integer one), and a close that hands return() no value
// completes a Generator<number[]> suspended in a finally with undefined.
function* sg(): Generator<number> { yield 1; yield 2 }
const it = sg()
it.next(); it.next()
const e = it.next()
console.log(e.value, e.done, typeof e.value)
const viaAny: any = sg()
viaAny.next(); viaAny.next()
const end: any = viaAny.next()
console.log(end.value, end.done)

function* fin(): Generator<number[]> { try { yield [1, 2]; yield [3, 4] } finally { console.log("closed") } }
function head() { for (const [x, y] of fin()) { console.log(x, y); if (x === 1) break } console.log("after") }
head()
const f: any = fin()
f.next()
const r = f.return()
console.log(r.value, r.done)
function* early(): Generator<number[]> { try { yield [1]; return } finally { console.log("fin") } }
const g = early()
g.next()
const last = g.next()
console.log(last.value, last.done)
