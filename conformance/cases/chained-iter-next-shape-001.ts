// `xs.values().next()` (no via-var) — the receiver of `.next()` is a
// Call expression, not an Ident. try_lower previously only inspected
// `ctx.locals` on Ident receivers → chained shapes fell through with
// "unsupported member call shape: next" (loud). Fix: prefer the
// checker-inferred type on the receiver expr; the Ident/locals lookup
// stays as fallback so via-var and pre-checker paths still resolve.
const arr: any[] = [1, 2, 3]
console.log(arr.values().next().value)
console.log(arr.keys().next().value, arr.keys().next().done)
console.log(arr.entries().next().value[1])

const typed: number[] = [10, 20]
console.log(typed.values().next().value)

const m = new Map<string, number>()
m.set('a', 1)
m.set('b', 2)
console.log(m.keys().next().value)
console.log(m.values().next().value)
console.log(JSON.stringify(m.entries().next().value))
