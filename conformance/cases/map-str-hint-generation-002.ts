// a string-key set takes its hint from an earlier find even across a
// call; the call here clears, deletes, refills and rehashes the Map
function bump(m: Map<string, number>, i: number): number {
  if (i % 7 === 3) m.clear()
  if (i % 5 === 1) m.delete('k' + ((i + 1) % 4))
  if (i % 11 === 4) {
    for (let j = 0; j < 40; j = j + 1) m.set('fill' + i + '_' + j, j)
  }
  if (i % 13 === 6) m.set('k' + ((i + 2) % 4), -1)
  return i
}

function run(n: number): string {
  const m = new Map<string, number>()
  for (let i = 0; i < n; i = i + 1) {
    const key = 'k' + (i % 4)
    if (m.has(key)) {
      const v = m.get(key)!
      const b = bump(m, i)
      m.set(key, v + b)
    } else {
      const b = bump(m, i)
      m.set(key, b)
    }
  }
  const out: string[] = []
  for (const [k, v] of m) {
    if (k.startsWith('k')) out.push(k + '=' + v)
  }
  return m.size + ' ' + out.join(' ')
}

console.log(run(parseInt(process.argv[2] ?? '60', 10)))

// the entry the find answered is wiped and its index refilled by
// another key before the set
function swap(m: Map<string, number>, k: string): number {
  m.clear()
  m.set(k, 100)
  return 1
}

function refill(a: string, b: string): string {
  const m = new Map<string, number>()
  m.set(a, 1)
  if (m.has(a)) {
    const v = m.get(a)!
    const w = swap(m, b)
    m.set(a, v + w)
  }
  const out: string[] = []
  for (const [k, v] of m) out.push(k + '=' + v)
  return out.join(' ')
}

console.log(refill('a' + process.argv.length, 'b' + process.argv.length))
