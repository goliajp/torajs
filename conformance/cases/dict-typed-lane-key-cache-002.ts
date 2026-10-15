// a write that hits the key-cell cache still honours an entry made
// read-only after the cache learned it: Object.freeze on the whole
// dictionary, and defineProperty writable: false on one entry
const keys: string[] = []
for (let i = 0; i < 8; i = i + 1) {
  keys.push("k" + i)
}
const d: Record<string, number> = {}
for (let round = 0; round < 3; round = round + 1) {
  for (const k of keys) {
    d[k] = (d[k] ?? 0) + 1
  }
}
console.log(d[keys[0]], d[keys[7]])

const e: Record<string, number> = {}
for (const k of keys) {
  e[k] = 1
}
for (const k of keys) {
  e[k] = e[k] + 1
}
Object.defineProperty(e, keys[2], { writable: false })
for (const k of keys) {
  try {
    e[k] = e[k] + 10
  } catch (err) {
    console.log("threw", k, err instanceof TypeError)
  }
}
console.log(e[keys[1]], e[keys[2]], e[keys[3]])

Object.freeze(d)
let threw = 0
for (const k of keys) {
  try {
    d[k] = 99
  } catch (err) {
    threw = threw + 1
  }
}
console.log(threw, d[keys[0]], d[keys[5]], Object.isFrozen(d))
