// A narrowed value that a call run since its test may have nulled,
// stored where no type tells how it is read back: a thrown value read
// back through a cast, of its own type and as `any`.

type N = { n: number }
let q: N | null = { n: 1 }
function f(): boolean { q = null; return true }
function report(e: unknown): void { console.log(e instanceof TypeError ? "TypeError" : "other") }

let caught: any = 0
if (q !== null && f()) { try { throw q } catch (e) { caught = e } }

q = { n: 2 }
let held: any = 0
if (q !== null && f()) { try { throw q as any } catch (e) { held = e } }
try { console.log((held as N).n) } catch (e) { report(e) }

// values of the same kind that no such value reaches
function clone(a: N): N { return { ...a } }
console.log(JSON.stringify(clone({ n: 7 })))
const words: string[] = ["a", "bc"]
let len = 0
for (const w of words) len += w.length
console.log(len)
console.log("end")
