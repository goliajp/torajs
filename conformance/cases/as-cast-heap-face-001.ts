// The typed tier materializes the face an `as` annotation names —
// it always did for the primitives (`x as number` unboxes), and
// never did for the heap shapes. So a value coming back out of
// `any` through a cast stayed a NaN-box while the checker had
// already answered `Map<string, number>`, and every lane that
// dispatches on the operand either declined the receiver it was
// written for or would have read a header off a boxed word:
// `(o.m as Map<string, number>).get("k")` was the compile error
// `unsupported member call shape: get`.
//
// Not covered, and recorded rather than papered over: `as T[]` and
// `as () => R`. Both need an interned element type or signature to
// name their SSA type, which is a different question from naming
// one heap layout.

const m = new Map([["k", 1]])
const s = new Set([7])
const o: any = { m, s, d: new Date(0), re: /a(b)/ }

// Cast receiver, straight through.
console.log((o.m as Map<string, number>).get("k"))
console.log((o.s as Set<number>).has(7))
console.log((o.d as Date).getTime())
console.log((o.re as RegExp).test("ab"))

// Through a binding — the same materialization, one statement later.
const m2 = o.m as Map<string, number>
console.log(m2.get("k"), m2.size)
const s2 = o.s as Set<number>
console.log(s2.has(7), s2.size)

// A cast that changes nothing is still identity.
console.log((m as Map<string, number>).get("k"))

// Iteration off a cast receiver.
const keys: string[] = []
for (const k of (o.m as Map<string, number>).keys()) keys.push(k)
console.log(keys.join(","))

// The weak collections take the same spelling.
const key: any = {}
const wm: any = new WeakMap()
wm.set(key, 3)
console.log((wm as WeakMap<object, number>).get(key))
const ws: any = new WeakSet()
ws.add(key)
console.log((ws as WeakSet<object>).has(key))

// Ownership follows the inner expression, not the cast: a call
// receiver is an owned temp either way.
function mk(): any { return new Map([["z", 9]]) }
console.log((mk() as Map<string, number>).get("z"))

let seen = 0
for (let i = 0; i < 2000; i++) {
  const one: any = new Map([["v", i]])
  if ((one as Map<string, number>).has("v")) seen++
  const two = mk() as Map<string, number>
  if (two.has("z")) seen++
}
console.log(seen)
