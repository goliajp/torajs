// Split products stored into the elements of a `string[][]` container
// (`rows[i] = parts`, `rows.push(parts)`) and read back through
// `const row = rows[j]`: every element read, measured, sliced, joined,
// iterated, copied into an owned array and used as a map key, with the
// heap reused in between so a dangling view would read garbage.
function next(s: number): number {
  const t = (s * 48271) % 2147483647
  return t === 0 ? 1 : t
}
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('wwwwwwwwwwww' + i)
}

let seed = 7
const lines: string[] = []
for (let i = 0; i < 16; i = i + 1) {
  let line = ''
  for (let f = 0; f < 5; f = f + 1) {
    seed = next(seed)
    if (f > 0) line = line + ','
    line = line + (seed % 100000).toString()
  }
  lines.push(line)
}
lines.push('x,yy,zzz')

const rows: string[][] = []
for (let i = 0; i < 8; i = i + 1) {
  rows.push([])
}
let total = 0
for (let i = 0; i < 100; i = i + 1) {
  const parts = lines[i % lines.length].split(',')
  let rebuilt = ''
  for (let k = 0; k < parts.length; k = k + 1) {
    const p = parts[k]
    total = total + p.charCodeAt(p.length - 1)
    rebuilt = rebuilt + p + ';'
  }
  total = total + rebuilt.length
  rows[i & 7] = parts
}
const tail = 'a,b,c'.split(',')
rows.push(tail)
const extra = lines[3].split(',')
rows.push(extra)
churn()

const owned: string[] = []
const seen = new Map<string, number>()
let held = 0
for (let j = 0; j < rows.length; j = j + 1) {
  const row = rows[j]
  held = held + row.length + row[row.length - 1].length + rows[j].length
  const first = row[0]
  owned.push(row[1])
  seen.set(first, (seen.get(first) ?? 0) + 1)
  let codes = 0
  for (const s of row) codes = codes + s.charCodeAt(0)
  console.log(j, row.join('|'), first.slice(1, 3), codes)
}
churn()
console.log(total, held, rows.length)
console.log(owned.join(','))
const r0 = rows[0]
console.log(seen.size, seen.get(r0[0]) ?? -1)
