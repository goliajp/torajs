// Closing an inferred-type generator from a for-of: break, return and
// throw out of the body run its finally, with array and number yields,
// a destructuring head, and a throw() resumed into a catch.
function* fin() { try { yield [1, 2]; yield [3, 4] } finally { console.log("closed") } }
function* nfin() { try { yield 1; yield 2 } finally { console.log("nclosed") } }

function a1() { for (const v of fin()) { console.log(v[0]); if (v[0] === 1) break } console.log("a1") }
a1()
function a2() { for (const [x, y] of fin()) { console.log(x, y); if (x === 1) break } console.log("a2") }
a2()
function a3(): number { for (const v of nfin()) { if (v === 1) return v * 10 } return -1 }
console.log(a3())
try {
  for (const v of nfin()) { if (v === 2) throw new Error("body " + v) }
} catch (e) {
  console.log((e as Error).message)
}
for (const [x, y] of fin()) { console.log(x + y) }
async function a4() {
  for (const [x, y] of fin()) { await null; console.log(x, y); if (x === 1) break }
  console.log("a4")
}
a4()

function* withThrow(k: number) { try { yield k; yield k + 1 } catch (e) { yield -1 } }
const wt = withThrow(3)
wt.next()
console.log(wt.throw(new Error("e")).value, wt.next().done)
