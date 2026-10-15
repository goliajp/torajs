// A getter defined with a literal key on an optional `number` member
// answers whatever it returns — null, a string, a boolean, undefined —
// and a read of the member sees that value: in `??`, in `typeof`, in
// `+` and `JSON.stringify`.
type Rec = { key: string; hits?: number; n: number }

const g: Rec = { key: 'g', n: 4 }
const answers: any[] = [null, 'str', 5, undefined, 0, false, 2.5]
let calls = 0
function dflt(): number {
  calls = calls + 1
  return 10
}
for (const a of answers) {
  Object.defineProperty(g, 'hits', { get: () => a, configurable: true, enumerable: true })
  const v = g.hits ?? 7
  const s = g.hits ?? 'fallback'
  console.log(typeof g.hits, typeof v, v, s, g.hits ?? dflt(), JSON.stringify([g.hits]))
}
console.log(calls)
Object.defineProperty(g, 'hits', { value: null, writable: true, configurable: true })
console.log(g.hits ?? dflt(), calls)
Object.defineProperty(g, 'hits', { value: 8.25, writable: true, configurable: true })
console.log(g.hits ?? dflt(), (g.hits ?? 0) + g.n, calls)
