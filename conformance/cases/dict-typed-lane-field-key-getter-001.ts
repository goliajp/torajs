// `d[r.key]` on a `Record<string, number>` where the record's `key`
// field itself has become an accessor: the getter hands back a fresh
// string each time, the read and the write each use the string their
// own key expression produced, and a temporary record whose field is
// an accessor works the same way.

type Rec = { key: string; n: number }

const d: Record<string, number> = {}
const r: Rec = { key: 'a', n: 1 }
let i = 0
Object.defineProperty(r, 'key', {
  get() {
    i = i + 1
    return 'k' + (i % 3)
  },
  configurable: true,
})
for (let j = 0; j < 9; j = j + 1) {
  d[r.key] = (d[r.key] ?? 0) + 1
}
console.log(JSON.stringify(d), i)
let hits = 0
for (let j = 0; j < 6; j = j + 1) {
  const v = d[r.key]
  if (v !== undefined) hits = hits + v
}
console.log(hits, i)

let made = 0
function mk(): Rec {
  const t: Rec = { key: 'x', n: 0 }
  Object.defineProperty(t, 'key', {
    get() {
      made = made + 1
      return 'm' + (made % 2)
    },
  })
  return t
}
const e: Record<string, number> = {}
for (let j = 0; j < 8; j = j + 1) {
  e[mk().key] = (e[mk().key] ?? 10) + j
}
console.log(JSON.stringify(e), made)
let sum = 0
for (let j = 0; j < 4; j = j + 1) {
  sum = sum + (e[mk().key] ?? 0)
}
console.log(sum, made)
