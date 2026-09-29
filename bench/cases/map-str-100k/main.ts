const m = new Map<string, number>()
const n = parseInt(process.argv[2], 10)
const keys = parseInt(process.argv[3], 10)
for (let i = 0; i < n; i = i + 1) {
  const j = i % keys
  const key = i % 2 === 0 ? "key" + j : "ключ" + j
  if (m.has(key)) {
    m.set(key, m.get(key)! + i)
  } else {
    m.set(key, i)
  }
}
let total = 0
for (const v of m.values()) {
  total = total + v
}
console.log(m.size, total)
