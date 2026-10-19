// A narrowed value that a call run since its test may have nulled,
// read back through a wider slot and a literal key.

class B { n = 1 }
class D extends B { m = 2 }
type N = { n: number }
let q: N | null = { n: 1 }
let d: D | null = new D()
function f(): boolean { q = null; return true }
function fd(): boolean { d = null; return true }
function report(e: unknown): void { console.log(e instanceof TypeError ? "TypeError" : "other") }

// an array of D read back as an array of B
const ds: D[] = [new D()]
if (d !== null && fd()) ds.push(d)
function total(xs: B[]): number { let t = 0; for (const x of xs) t += x.n; return t }
const bs: B[] = ds
try { console.log(total(bs)) } catch (e) { report(e) }

// a map of D read back as a map of B
d = new D()
const md: Map<string, D> = new Map()
if (d !== null && fd()) md.set("a", d)
const mb: Map<string, B> = md
const v = mb.get("a")
try { if (v !== undefined) console.log(v.n) } catch (e) { report(e) }

// a property spelled as a literal key that is no identifier
q = { n: 4 }
const o2 = { "a-b": { n: 2 } }
if (q !== null && f()) o2["a-b"] = q
try { console.log(o2["a-b"].n) } catch (e) { report(e) }
// a numbered field of a class instance
q = { n: 6 }
class Slots { 0: N = { n: 2 } }
const sl = new Slots()
if (q !== null && f()) sl[0] = q
const got = sl[0]
try { console.log(got.n) } catch (e) { report(e) }
const before = { ...o2 }
console.log(Object.keys(before).length)
console.log("end")
