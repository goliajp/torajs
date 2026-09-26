// An arrow or function expression whose every path throws has type
// `() => never` (tsc infers it; bottom fits every return slot), so it
// is a legal comparator / mapper / method value. §23.1.3.30 step 1 only
// asks the comparator to be callable; the throw it raises propagates.
const xs: number[] = [3, 1, 2]
try {
  xs.sort(() => { throw new Error("cmp") })
} catch (e) {
  console.log("sort", (e as Error).message)
}
let calls = 0
try {
  [5, 4, 3, 2].toSorted((a, b) => { calls++; throw new RangeError("stop") })
} catch (e) {
  console.log("toSorted", e instanceof RangeError, calls)
}
const fail = (m: string) => { throw new TypeError(m) }
try {
  const n: number = fail("as number")
  console.log("unreached", n)
} catch (e) {
  console.log("typed slot", (e as Error).message)
}
try {
  const s: string = fail("as string")
  console.log("unreached", s)
} catch (e) {
  console.log("typed slot", (e as Error).message)
}
const pick = function (c: boolean) {
  if (c) throw new Error("left")
  else throw new Error("right")
}
for (const c of [true, false]) {
  try { pick(c) } catch (e) { console.log("branches", (e as Error).message) }
}
const guarded = () => {
  try { throw new Error("inner") } finally { console.log("finally ran") }
}
try { guarded() } catch (e) { console.log("after finally", (e as Error).message) }
const it = { next: () => ({ done: true, value: 0 }), return: () => ({ done: true, value: 1 }) }
it.return = () => { throw new Error("no return") }
try { it.return() } catch (e) { console.log("slot", (e as Error).message) }
const empty: number[] = []
console.log("empty", empty.map(() => { throw new Error("never called") }).length)
const annotated = (): never => { throw new Error("explicit") }
try { annotated() } catch (e) { console.log("annotated", (e as Error).message) }
const spin = (): never => { for (;;) { throw new Error("loop") } }
try { spin() } catch (e) { console.log("loop", (e as Error).message) }
const rej = async () => { throw new Error("async") }
rej().catch((e) => console.log("promise", (e as Error).message))
try {
  [1, 2].flatMap(() => { throw new Error("fm") })
} catch (e) { console.log("flatMap", (e as Error).message) }
console.log("empty", [].flatMap(() => { throw new Error("x") }).length)
