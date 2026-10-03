const m = new Map<string, number>()
const n = parseInt(process.argv[2] ?? '50', 10)

function bump(i: number): number {
  // writes the Map between the find and the set: clears, refills,
  // and grows it past a rehash
  if (i % 7 === 3) m.clear()
  if (i % 5 === 1) m.delete('k' + ((i + 1) % 4))
  if (i % 11 === 4) {
    for (let j = 0; j < 40; j = j + 1) m.set('fill' + i + '_' + j, j)
  }
  if (i % 13 === 6) m.set('k' + ((i + 2) % 4), -1)
  return i
}

for (let i = 0; i < n; i = i + 1) {
  const key = 'k' + (i % 4)
  if (m.has(key)) {
    m.set(key, m.get(key)! + bump(i))
  } else {
    m.set(key, bump(i))
  }
}
const out: string[] = []
for (const [k, v] of m) {
  if (k.startsWith('k')) out.push(k + '=' + v)
}
console.log(m.size, out.join(' '))
