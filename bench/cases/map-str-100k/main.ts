const m = new Map<string, number>()
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const j = i % 4096
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
